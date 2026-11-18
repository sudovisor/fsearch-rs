//! The FSearch window (egui, ADR-010). Port of `fsearch.c`, `fsearch_window.c`,
//! `fsearch_window_actions.c`, `fsearch_result_view.c`, `fsearch_list_view.c`,
//! `fsearch_statusbar.c` and `fsearch_listview_popup.c`, redesigned (ADR-019): looks in
//! `theme.rs`, drawn widgets in `widgets.rs`.

pub mod backend;
pub mod format;
pub mod ipc;
pub mod monitor;
mod prefs;
#[cfg(test)]
mod tests;
pub mod theme;
pub mod widgets;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Color32, FontData, Key, Modifiers, RichText, Stroke, text::LayoutJob, vec2,
};
use egui_extras::{Column, TableBuilder};
use fsearch_core::SortDir;
use fsearch_core::config::Config;
use fsearch_core::db::{Id, Prop};
use fsearch_core::query::MatchData;

use backend::{Backend, Event, Request, Results, SearchRequest};
use theme::{Chrome, Look, Palette};
use widgets::Icon;

pub const VERSION: &str = "0.3";

/// Opening more items than this at once asks first (each one starts an application).
const OPEN_CONFIRM_ABOVE: usize = 10;

pub struct StartOptions {
    pub search: Option<String>,
    pub minimized: bool,
    pub preferences: bool,
    pub update: bool,
}

#[derive(PartialEq)]
enum DbState {
    Loading,
    Scanning,
    Ready(usize),
}

/// Every shortcut, shown in menus, tooltips and the Shortcuts page of the preferences.
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("Focus the search field", "Ctrl+F"),
    ("Match case", "Ctrl+I"),
    ("Regular expressions", "Ctrl+R"),
    ("Search in path", "Ctrl+U"),
    ("Update the index", "Ctrl+Shift+R"),
    ("Preferences", "Ctrl+P"),
    ("Open selection", "Enter"),
    ("Open containing folder", "Ctrl+Enter"),
    ("Copy path", "Ctrl+C"),
    ("Select all results", "Ctrl+A"),
    ("Move to trash", "Delete"),
    ("Move in the list", "↑ ↓ PgUp PgDn Home End"),
    ("Search syntax help", "F1"),
    ("Hide or quit", "Esc"),
    ("Quit", "Ctrl+Q"),
];

/// Bytes of a stored path to an OS path.
pub fn path_from_bytes(b: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        PathBuf::from(std::ffi::OsStr::from_bytes(b))
    }
    #[cfg(not(unix))]
    {
        // stored with `/` separators (ADR-022); Windows APIs get its own
        PathBuf::from(String::from_utf8_lossy(b).replace('/', "\\"))
    }
}

type Fallbacks = Arc<Mutex<Option<Vec<(String, Arc<FontData>)>>>>;

struct App {
    config: Config,
    look: Look,
    pal: Palette,
    fallback_fonts: Fallbacks,
    fonts_with_fallbacks: bool,
    backend: Backend,
    ipc: Receiver<ipc::Message>,
    query_text: String,
    filter_idx: usize,
    results: Option<Results>,
    next_id: u64,
    sent: Option<(String, usize, u32, Prop, SortDir)>,
    sort: (Prop, SortDir),
    selection: HashSet<Id>,
    cursor: Option<usize>,
    anchor: Option<usize>,
    scroll_to: Option<usize>,
    focus_search: bool,
    search_has_focus: bool,
    db: DbState,
    task: Option<&'static str>,
    prefs: Option<prefs::Prefs>,
    about: bool,
    confirm_trash: Option<Vec<PathBuf>>,
    confirm_open: Option<(usize, bool, bool)>,
    error: Option<String>,
    monitor: Option<monitor::Monitor>,
    periodic: Option<Arc<std::sync::atomic::AtomicBool>>,
    pending_search_at: Option<Instant>,
    /// Test hook: `FSEARCH_SCREENSHOT=<png>` saves one frame once results are shown, then exits.
    screenshot: Option<(String, u32)>,
    screenshot_ready_at: Option<Instant>,
}

fn sort_from_name(s: &str) -> Prop {
    match s {
        "Path" => Prop::Path,
        "Size" => Prop::Size,
        "Date Modified" => Prop::Mtime,
        "Type" => Prop::FileType,
        "Extension" => Prop::Extension,
        _ => Prop::Name,
    }
}

fn sort_name(p: Prop) -> &'static str {
    match p {
        Prop::Path => "Path",
        Prop::Size => "Size",
        Prop::Mtime => "Date Modified",
        Prop::FileType => "Type",
        Prop::Extension => "Extension",
        _ => "Name",
    }
}

enum RowAction {
    Click(usize, Modifiers, bool, bool),
    Open,
    OpenFolder,
    Copy(u8),
    Trash,
}

impl App {
    fn new(cc: &eframe::CreationContext, config: Config, opts: StartOptions) -> App {
        let look = Look::from_id(&config.look);
        theme::apply(&cc.egui_ctx, look);
        cc.egui_ctx.set_fonts(theme::fonts(look, &[]));
        // wide-coverage fonts (CJK …) are large: read them off the UI thread
        let fallback_fonts: Fallbacks = Arc::new(Mutex::new(None));
        {
            let slot = fallback_fonts.clone();
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                let loaded: Vec<(String, Arc<FontData>)> = theme::load_fallback_fonts()
                    .into_iter()
                    .map(|(n, b, idx)| {
                        let mut fd = FontData::from_owned(b);
                        fd.index = idx;
                        (n, Arc::new(fd))
                    })
                    .collect();
                if let Ok(mut s) = slot.lock() {
                    *s = Some(loaded);
                }
                ctx.request_repaint();
            });
        }
        let ctx = cc.egui_ctx.clone();
        let backend = backend::start(
            config.clone(),
            opts.update,
            Box::new(move || ctx.request_repaint()),
        );
        let (itx, irx) = std::sync::mpsc::channel();
        let ctx2 = cc.egui_ctx.clone();
        ipc::listen(itx, Box::new(move || ctx2.request_repaint()));
        let sort = if config.restore_sort_order {
            (
                sort_from_name(config.sort_by.as_deref().unwrap_or("Name")),
                if config.sort_ascending {
                    SortDir::Ascending
                } else {
                    SortDir::Descending
                },
            )
        } else {
            (Prop::Name, SortDir::Ascending)
        };
        let ctx3 = cc.egui_ctx.clone();
        let monitor = monitor::start(
            &config.includes,
            backend.tx.clone(),
            Arc::new(move || ctx3.request_repaint()),
        );
        let periodic = monitor::start_periodic(&config.includes, backend.tx.clone());
        let mut app = App {
            prefs: opts.preferences.then(|| prefs::Prefs::new(&config)),
            pal: look.palette(),
            look,
            fallback_fonts,
            fonts_with_fallbacks: false,
            config,
            backend,
            ipc: irx,
            query_text: opts.search.unwrap_or_default(),
            filter_idx: 0,
            results: None,
            next_id: 1,
            sent: None,
            sort,
            selection: HashSet::new(),
            cursor: None,
            anchor: None,
            scroll_to: None,
            focus_search: true,
            search_has_focus: false,
            db: DbState::Loading,
            task: None,
            about: false,
            confirm_trash: None,
            confirm_open: None,
            error: None,
            monitor: Some(monitor),
            periodic: Some(periodic),
            pending_search_at: None,
            screenshot: std::env::var("FSEARCH_SCREENSHOT").ok().map(|p| (p, 0)),
            screenshot_ready_at: None,
        };
        if opts.minimized {
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        app.search(true);
        app
    }

    fn set_look(&mut self, ctx: &egui::Context, look: Look) {
        self.config.look = look.id().to_string();
        self.show_look(ctx, look);
    }

    /// Change what is shown without changing the setting (the preferences' live preview).
    fn show_look(&mut self, ctx: &egui::Context, look: Look) {
        self.look = look;
        self.pal = look.palette();
        theme::apply(ctx, look);
        let fb = self
            .fallback_fonts
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .unwrap_or_default();
        ctx.set_fonts(theme::fonts(look, &fb));
    }

    fn flags(&self) -> u32 {
        self.config.query_flags()
    }

    /// Queue a search when the query, filter, flags or sort changed (or `force`).
    fn search(&mut self, force: bool) {
        let key = (
            self.query_text.clone(),
            self.filter_idx,
            self.flags(),
            self.sort.0,
            self.sort.1,
        );
        if !force && self.sent.as_ref() == Some(&key) {
            return;
        }
        self.sent = Some(key);
        self.backend.new_search_token();
        let id = self.next_id;
        self.next_id += 1;
        let filters = self.config.filters.clone();
        let filter = filters.get(self.filter_idx).cloned();
        let _ = self.backend.tx.send(Request::Search(SearchRequest {
            id,
            text: self.query_text.clone(),
            filter,
            filters,
            flags: self.flags(),
            sort: self.sort.0,
            dir: self.sort.1,
        }));
    }

    fn set_sort(&mut self, prop: Prop) {
        let dir = if self.sort.0 == prop {
            if self.sort.1 == SortDir::Ascending {
                SortDir::Descending
            } else {
                SortDir::Ascending
            }
        } else {
            SortDir::Ascending
        };
        self.sort = (prop, dir);
        if let Some((t, f, fl, _, _)) = self.sent.clone() {
            self.sent = Some((t, f, fl, prop, dir));
        }
        let _ = self.backend.tx.send(Request::Sort {
            id: self.next_id,
            prop,
            dir,
        });
        self.next_id += 1;
        self.config.sort_by = Some(sort_name(prop).into());
        self.config.sort_ascending = dir == SortDir::Ascending;
    }

    fn query_is_empty(&self) -> bool {
        fsearch_string_utils_rs::port::fsearch_string_is_empty(Some(self.query_text.as_bytes()))
    }

    fn visible_len(&self) -> usize {
        if self.config.hide_results_on_empty_search && self.query_is_empty() {
            return 0;
        }
        self.results.as_ref().map_or(0, |r| r.view.len())
    }

    fn entry_at(&self, i: usize) -> Option<Id> {
        if i >= self.visible_len() {
            return None;
        }
        self.results.as_ref().and_then(|r| r.view.get(i))
    }

    /// Selected entries in list order.
    fn selected_ids(&self) -> Vec<Id> {
        let Some(r) = &self.results else {
            return Vec::new();
        };
        if self.selection.is_empty() {
            return Vec::new();
        }
        (0..self.visible_len())
            .filter_map(|i| r.view.get(i))
            .filter(|id| self.selection.contains(id))
            .collect()
    }

    fn selected_paths(&self) -> Vec<PathBuf> {
        let Some(r) = &self.results else {
            return Vec::new();
        };
        self.selected_ids()
            .into_iter()
            .map(|id| path_from_bytes(&r.store.db.full_path(id)))
            .collect()
    }

    fn open_paths(&mut self, ctx: &egui::Context, folders: bool, keyboard: bool) {
        let paths = self.selected_paths();
        if paths.len() > OPEN_CONFIRM_ABOVE && self.confirm_open.is_none() {
            self.confirm_open = Some((paths.len(), folders, keyboard));
            return;
        }
        self.open_confirmed(ctx, paths, folders, keyboard);
    }

    fn open_confirmed(
        &mut self,
        ctx: &egui::Context,
        paths: Vec<PathBuf>,
        folders: bool,
        keyboard: bool,
    ) {
        for p in &paths {
            let target = if folders {
                p.parent()
                    .map(|x| x.to_path_buf())
                    .unwrap_or_else(|| p.clone())
            } else {
                p.clone()
            };
            let res = if folders
                && self
                    .config
                    .folder_open_cmd
                    .as_deref()
                    .is_some_and(|c| !c.trim().is_empty())
            {
                let cmd = self.config.folder_open_cmd.clone().unwrap_or_default();
                let mut parts = cmd.split_whitespace();
                match parts.next() {
                    Some(prog) => std::process::Command::new(prog)
                        .args(parts)
                        .arg(p)
                        .spawn()
                        .map(|_| ()),
                    None => open::that_detached(&target),
                }
            } else {
                open::that_detached(&target)
            };
            if let Err(e) = res
                && self.config.show_dialog_failed_opening
            {
                self.error = Some(format!("Could not open {}: {e}", target.display()));
            }
        }
        if !paths.is_empty() {
            let after = if keyboard {
                self.config.action_after_file_open_keyboard
            } else {
                self.config.action_after_file_open_mouse
            };
            if after {
                match self.config.action_after_file_open {
                    1 => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                    2 => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                    _ => {}
                }
            }
        }
    }

    fn copy(&self, what: u8) {
        let Some(r) = &self.results else { return };
        let lines: Vec<String> = self
            .selected_ids()
            .into_iter()
            .map(|id| {
                let b = match what {
                    0 => r.store.db.display_name(id).to_vec(),
                    1 => r.store.db.parent_path(id),
                    _ => r.store.db.full_path(id),
                };
                String::from_utf8_lossy(&b).into_owned()
            })
            .collect();
        if lines.is_empty() {
            return;
        }
        if let Ok(mut cb) = arboard::Clipboard::new() {
            let _ = cb.set_text(lines.join("\n"));
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        if !self.fonts_with_fallbacks
            && let Some(fb) = self.fallback_fonts.lock().ok().and_then(|g| g.clone())
        {
            self.fonts_with_fallbacks = true;
            if !fb.is_empty() {
                ctx.set_fonts(theme::fonts(self.look, &fb));
            }
        }
        while let Ok(ev) = self.backend.rx.try_recv() {
            match ev {
                Event::Loading => self.db = DbState::Loading,
                Event::Scanning => self.db = DbState::Scanning,
                Event::Ready { folders, files } => {
                    self.db = DbState::Ready(folders + files);
                    self.task = None;
                }
                Event::Querying => self.task = Some("Searching"),
                Event::Sorting => self.task = Some("Sorting"),
                Event::Results(r) => {
                    self.task = None;
                    // Benchmark hook: wall-clock time of the first results, then quit.
                    if self.results.is_none()
                        && std::env::var_os("FSEARCH_STARTUP_TIMING").is_some()
                    {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default();
                        eprintln!("first-results {:.6}", now.as_secs_f64());
                        std::process::exit(0);
                    }
                    // a new index invalidates the selected entry ids
                    if let Some(old) = &self.results
                        && !Arc::ptr_eq(&old.store, &r.store)
                    {
                        self.selection.clear();
                        self.anchor = None;
                    }
                    self.cursor = self.cursor.filter(|&c| c < r.view.len());
                    self.anchor = self.anchor.filter(|&c| c < r.view.len());
                    self.results = Some(r);
                }
                Event::Saved => {}
            }
        }
        while let Ok(m) = self.ipc.try_recv() {
            match m {
                ipc::Message::Search(t) => {
                    self.query_text = t;
                    self.search(true);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                ipc::Message::Preferences => self.prefs = Some(prefs::Prefs::new(&self.config)),
                ipc::Message::Update => {
                    let _ = self.backend.tx.send(Request::Rescan);
                }
                ipc::Message::Activate => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
            }
        }
    }

    fn apply_config(&mut self, ctx: &egui::Context, c: Config) {
        let _ = c.save();
        self.monitor = None;
        if let Some(p) = self.periodic.take() {
            p.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let _ = self
            .backend
            .tx
            .send(Request::SetConfig(Box::new(c.clone())));
        let look = Look::from_id(&c.look);
        self.config = c;
        if look != self.look {
            self.set_look(ctx, look);
        }
        let ctx2 = ctx.clone();
        self.monitor = Some(monitor::start(
            &self.config.includes,
            self.backend.tx.clone(),
            Arc::new(move || ctx2.request_repaint()),
        ));
        self.periodic = Some(monitor::start_periodic(
            &self.config.includes,
            self.backend.tx.clone(),
        ));
        if self.filter_idx >= self.config.filters.len() {
            self.filter_idx = 0;
        }
        self.search(true);
    }

    fn add_folder(&mut self, ctx: &egui::Context) {
        if let Some(p) = rfd::FileDialog::new()
            .set_title("Add a folder to the index")
            .pick_folder()
        {
            let mut c = self.config.clone();
            fsearch_core::rules::add_include(
                &mut c.includes,
                fsearch_core::rules::Include::new(
                    &p.to_string_lossy(),
                    true,
                    false,
                    false,
                    false,
                    0,
                ),
            );
            self.apply_config(ctx, c);
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = |k: Key| egui::KeyboardShortcut::new(Modifiers::COMMAND, k);
        let cmd_shift =
            |k: Key| egui::KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, k);
        #[derive(PartialEq)]
        enum A {
            None,
            Case,
            Regex,
            Path,
            Update,
            Prefs,
            Quit,
            Help,
            SelectAll,
            Copy,
            OpenFolder,
        }
        let mut act = A::None;
        let typing = self.search_has_focus;
        ctx.input_mut(|i| {
            // Ctrl+Shift+R before Ctrl+R: egui's shortcut match ignores an extra Shift
            if i.consume_shortcut(&cmd_shift(Key::R)) {
                act = A::Update;
            } else if i.consume_shortcut(&cmd(Key::R)) {
                act = A::Regex;
            } else if i.consume_shortcut(&cmd(Key::F)) {
                self.focus_search = true;
            } else if i.consume_shortcut(&cmd(Key::I)) {
                act = A::Case;
            } else if i.consume_shortcut(&cmd(Key::U)) {
                act = A::Path;
            } else if i.consume_shortcut(&cmd(Key::P)) {
                act = A::Prefs;
            } else if i.consume_shortcut(&cmd(Key::Q)) || i.consume_shortcut(&cmd(Key::W)) {
                act = A::Quit;
            } else if i.consume_key(Modifiers::NONE, Key::F1) {
                act = A::Help;
            } else if !typing && i.consume_shortcut(&cmd(Key::A)) {
                act = A::SelectAll;
            } else if !typing
                && (i.consume_shortcut(&cmd(Key::C))
                    || i.events.iter().any(|e| matches!(e, egui::Event::Copy)))
            {
                act = A::Copy;
            } else if !typing && i.consume_shortcut(&cmd(Key::Enter)) {
                act = A::OpenFolder;
            }
        });
        match act {
            A::Case => self.config.match_case = !self.config.match_case,
            A::Regex => self.config.enable_regex = !self.config.enable_regex,
            A::Path => self.config.search_in_path = !self.config.search_in_path,
            A::Update => {
                let _ = self.backend.tx.send(Request::Rescan);
            }
            A::Prefs => self.prefs = Some(prefs::Prefs::new(&self.config)),
            A::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            A::Help => {
                let _ = open::that_detached(
                    "https://github.com/cboxdoerfer/fsearch/wiki/Search-syntax",
                );
            }
            A::SelectAll => self.select_all(),
            A::Copy => self.copy(2),
            A::OpenFolder => self.open_paths(ctx, true, true),
            A::None => {}
        }
        if ctx.input(|i| i.key_pressed(Key::Escape))
            && self.prefs.is_none()
            && !self.about
            && self.confirm_trash.is_none()
        {
            if self.config.exit_on_escape {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
        }
    }

    fn menu_item(ui: &mut egui::Ui, label: &str, shortcut: &str) -> bool {
        let mut b = if shortcut.is_empty() {
            egui::Button::new(label)
        } else {
            egui::Button::new(label).shortcut_text(shortcut)
        };
        if !ui.is_enabled() {
            b = b.frame(false);
        }
        let clicked = ui.add(b).clicked();
        if clicked {
            ui.close();
        }
        clicked
    }

    fn menu_toggle(
        ui: &mut egui::Ui,
        pal: &Palette,
        on: &mut bool,
        label: &str,
        shortcut: &str,
    ) -> bool {
        let color = if pal.bevel { pal.text } else { pal.accent };
        let mut b = egui::Button::new((widgets::check_atom(*on, color), label));
        if !shortcut.is_empty() {
            b = b.shortcut_text(shortcut);
        }
        if !ui.is_enabled() {
            b = b.frame(false);
        }
        let resp = ui.add(b);
        let clicked = resp.clicked();
        if clicked {
            *on = !*on;
        }
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), *on, label)
        });
        clicked
    }

    /// The commands, shared by the menu bar and the header-bar menu.
    fn menus(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.menu_button("File", |ui| {
            if Self::menu_item(ui, "Add folder…", "") {
                self.add_folder(ctx);
            }
            if Self::menu_item(ui, "Update index", "Ctrl+Shift+R") {
                let _ = self.backend.tx.send(Request::Rescan);
            }
            ui.separator();
            if Self::menu_item(ui, "Quit", "Ctrl+Q") {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        ui.menu_button("Edit", |ui| {
            let any = !self.selection.is_empty();
            ui.add_enabled_ui(any, |ui| {
                if Self::menu_item(ui, "Open", "Enter") {
                    self.open_paths(ctx, false, false);
                }
                if Self::menu_item(ui, "Open containing folder", "Ctrl+Enter") {
                    self.open_paths(ctx, true, false);
                }
                ui.separator();
                if Self::menu_item(ui, "Copy name", "") {
                    self.copy(0);
                }
                if Self::menu_item(ui, "Copy path", "Ctrl+C") {
                    self.copy(2);
                }
                if Self::menu_item(ui, "Move to trash", "Delete") {
                    self.confirm_trash = Some(self.selected_paths());
                }
            });
            ui.separator();
            if Self::menu_item(ui, "Select all", "Ctrl+A") {
                self.select_all();
            }
            if Self::menu_item(ui, "Invert selection", "") {
                self.invert_selection();
            }
            if Self::menu_item(ui, "Clear selection", "") {
                self.selection.clear();
            }
            ui.separator();
            if Self::menu_item(ui, "Preferences", "Ctrl+P") {
                self.prefs = Some(prefs::Prefs::new(&self.config));
            }
        });
        let mut toggled = false;
        let p = self.pal;
        let check_color = if p.bevel { p.text } else { p.accent };
        ui.menu_button("View", |ui| {
            toggled |= Self::menu_toggle(ui, &p, &mut self.config.show_statusbar, "Status bar", "");
            toggled |= Self::menu_toggle(ui, &p, &mut self.config.show_filter, "Filter", "");
            if self.pal.chrome == Chrome::Menubar {
                toggled |= Self::menu_toggle(ui, &p, &mut self.config.show_menubar, "Menu bar", "");
            }
            ui.separator();
            toggled |=
                Self::menu_toggle(ui, &p, &mut self.config.show_path_column, "Path column", "");
            toggled |=
                Self::menu_toggle(ui, &p, &mut self.config.show_type_column, "Type column", "");
            toggled |= Self::menu_toggle(
                ui,
                &p,
                &mut self.config.show_extension_column,
                "Extension column",
                "",
            );
            toggled |=
                Self::menu_toggle(ui, &p, &mut self.config.show_size_column, "Size column", "");
            toggled |= Self::menu_toggle(
                ui,
                &p,
                &mut self.config.show_modified_column,
                "Modified column",
                "",
            );
            ui.separator();
            ui.menu_button((widgets::check_atom(false, check_color), "Look"), |ui| {
                for l in theme::ALL {
                    let on = self.look == l;
                    let check = widgets::check_atom(on, check_color);
                    let mut b = egui::Button::new((check, l.name()));
                    if !ui.is_enabled() {
                        b = b.frame(false);
                    }
                    let resp = ui.add(b);
                    resp.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::Role::Button,
                            ui.is_enabled(),
                            on,
                            l.name(),
                        )
                    });
                    if resp.clicked() {
                        self.set_look(ui.ctx(), l);
                        if !Config::unreadable() {
                            let mut c = Config::load_or_default();
                            c.look = l.id().into();
                            let _ = c.save();
                        }
                        ui.close();
                    }
                }
            });
        });
        ui.menu_button("Search", |ui| {
            toggled |=
                Self::menu_toggle(ui, &p, &mut self.config.match_case, "Match case", "Ctrl+I");
            toggled |= Self::menu_toggle(
                ui,
                &p,
                &mut self.config.enable_regex,
                "Regular expressions",
                "Ctrl+R",
            );
            toggled |= Self::menu_toggle(
                ui,
                &p,
                &mut self.config.search_in_path,
                "Search in path",
                "Ctrl+U",
            );
            ui.separator();
            toggled |= Self::menu_toggle(
                ui,
                &p,
                &mut self.config.auto_match_case,
                "Match case when the query has capitals",
                "",
            );
            toggled |= Self::menu_toggle(
                ui,
                &p,
                &mut self.config.auto_search_in_path,
                "Search in path when the query has /",
                "",
            );
        });
        if toggled {
            self.persist_config();
        }
        ui.menu_button("Help", |ui| {
            if Self::menu_item(ui, "Search syntax", "F1") {
                let _ = open::that_detached(
                    "https://github.com/cboxdoerfer/fsearch/wiki/Search-syntax",
                );
            }
            if Self::menu_item(ui, "Keyboard shortcuts", "") {
                let mut p = prefs::Prefs::new(&self.config);
                p.show_shortcuts();
                self.prefs = Some(p);
            }
            if Self::menu_item(ui, "About FSearch", "") {
                self.about = true;
            }
        });
    }

    fn select_all(&mut self) {
        for i in 0..self.visible_len() {
            if let Some(id) = self.entry_at(i) {
                self.selection.insert(id);
            }
        }
    }

    fn invert_selection(&mut self) {
        for i in 0..self.visible_len() {
            if let Some(id) = self.entry_at(i)
                && !self.selection.remove(&id)
            {
                self.selection.insert(id);
            }
        }
    }

    fn click_row(&mut self, i: usize, mods: Modifiers) {
        let Some(id) = self.entry_at(i) else { return };
        if mods.shift {
            let a = self.anchor.unwrap_or(i);
            if !mods.command {
                self.selection.clear();
            }
            for k in a.min(i)..=a.max(i) {
                if let Some(e) = self.entry_at(k) {
                    self.selection.insert(e);
                }
            }
        } else if mods.command {
            if !self.selection.remove(&id) {
                self.selection.insert(id);
            }
            self.anchor = Some(i);
        } else {
            self.selection.clear();
            self.selection.insert(id);
            self.anchor = Some(i);
        }
        self.cursor = Some(i);
    }

    fn list_keys(&mut self, ctx: &egui::Context, rows_per_page: usize) {
        let n = self.visible_len();
        if n == 0 {
            return;
        }
        let (down, up, pgdn, pgup, home, end, enter, del, mods) = ctx.input(|i| {
            (
                i.key_pressed(Key::ArrowDown),
                i.key_pressed(Key::ArrowUp),
                i.key_pressed(Key::PageDown),
                i.key_pressed(Key::PageUp),
                i.key_pressed(Key::Home),
                i.key_pressed(Key::End),
                i.key_pressed(Key::Enter) && !i.modifiers.command,
                i.key_pressed(Key::Delete),
                i.modifiers,
            )
        });
        let page = rows_per_page.max(1);
        let cur = self.cursor.unwrap_or(0).min(n - 1);
        let target = if down {
            Some(if self.cursor.is_none() {
                0
            } else {
                (cur + 1).min(n - 1)
            })
        } else if up {
            Some(cur.saturating_sub(1))
        } else if pgdn {
            Some((cur + page).min(n - 1))
        } else if pgup {
            Some(cur.saturating_sub(page))
        } else if home {
            Some(0)
        } else if end {
            Some(n - 1)
        } else {
            None
        };
        if let Some(t) = target {
            self.click_row(
                t,
                Modifiers {
                    shift: mods.shift,
                    ..Default::default()
                },
            );
            self.scroll_to = Some(t);
        }
        if up && cur == 0 && self.cursor == Some(0) {
            self.focus_search = true;
        }
        if enter && !self.selection.is_empty() {
            self.open_paths(ctx, false, true);
        }
        if del && !self.selection.is_empty() {
            self.confirm_trash = Some(self.selected_paths());
        }
    }

    fn name_job(&self, r: &Results, id: Id, md: &mut MatchData, selected: bool) -> LayoutJob {
        let p = &self.pal;
        let name = String::from_utf8_lossy(r.store.db.display_name(id)).into_owned();
        let filled = selected && !p.sel_marker;
        let fg = if filled { p.sel_fg } else { p.text };
        let font = egui::FontId::proportional(p.body_size);
        let normal = egui::TextFormat {
            font_id: font.clone(),
            color: fg,
            ..Default::default()
        };
        let marked = if filled {
            egui::TextFormat {
                font_id: font,
                color: p.sel_fg,
                underline: Stroke::new(1.0, p.sel_fg),
                ..Default::default()
            }
        } else {
            egui::TextFormat {
                font_id: font,
                color: p.match_fg,
                background: p.match_bg,
                ..Default::default()
            }
        };
        let mut ranges = Vec::new();
        if self.config.highlight_search_terms && !self.query_is_empty() {
            md.set(id);
            ranges = r.query.highlight(md).name;
            ranges.sort();
        }
        let mut job = LayoutJob::default();
        let mut pos = 0usize;
        for (s, e) in ranges {
            let (s, e) = (s.min(name.len()), e.min(name.len()));
            if s < pos || s >= e || !name.is_char_boundary(s) || !name.is_char_boundary(e) {
                continue;
            }
            job.append(&name[pos..s], 0.0, normal.clone());
            job.append(&name[s..e], 0.0, marked.clone());
            pos = e;
        }
        job.append(&name[pos..], 0.0, normal);
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        job.wrap.overflow_character = Some('…');
        job
    }

    /// Empty states: no folders yet, still loading, or nothing found.
    fn empty_state(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> bool {
        let p = self.pal;
        let total = match self.db {
            DbState::Ready(n) => Some(n),
            _ => None,
        };
        let rect = ui.available_rect_before_wrap();
        let mid = rect.center().y - 60.0;
        if self.config.includes.is_empty() && total.unwrap_or(0) == 0 && self.db != DbState::Loading
        {
            let mut y = mid;
            y += widgets::centered_text(
                ui,
                rect,
                y,
                "No folders indexed yet",
                p.body_size + 4.0,
                p.text,
            ) + 8.0;
            y += widgets::centered_text(
                ui,
                rect,
                y,
                "Add a folder and FSearch indexes every name in it, so results appear as you type",
                p.body_size,
                p.text2,
            ) + 16.0;
            let bw = 280.0;
            let area = egui::Rect::from_min_size(
                egui::pos2(rect.center().x - bw / 2.0, y),
                vec2(bw, 32.0),
            );
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(area)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    let primary =
                        egui::Button::new(RichText::new("Add folder…").color(p.on_accent))
                            .fill(p.accent)
                            .min_size(vec2(128.0, 32.0));
                    if ui.add(primary).clicked() {
                        self.add_folder(ctx);
                    }
                    if ui
                        .add(egui::Button::new("Preferences").min_size(vec2(128.0, 32.0)))
                        .clicked()
                    {
                        let mut pr = prefs::Prefs::new(&self.config);
                        pr.show_database();
                        self.prefs = Some(pr);
                    }
                },
            );
            return true;
        }
        if self.results.is_none()
            || (self.db == DbState::Loading && total.is_none() && self.visible_len() == 0)
        {
            let s = if self.db == DbState::Scanning {
                "Building the index"
            } else {
                "Loading the index"
            };
            widgets::centered_text(ui, rect, mid + 24.0, s, p.body_size, p.text2);
            let sp =
                egui::Rect::from_center_size(egui::pos2(rect.center().x, mid), vec2(20.0, 20.0));
            ui.put(sp, egui::Spinner::new().color(p.muted));
            return true;
        }
        if self.visible_len() == 0 {
            if self.config.hide_results_on_empty_search && self.query_is_empty() {
                let n = total.map(format::count).unwrap_or_default();
                widgets::centered_text(
                    ui,
                    rect,
                    mid,
                    &format!("Type to search {n} indexed items"),
                    p.body_size,
                    p.text2,
                );
                return true;
            }
            if self.task.is_none() {
                let mut y = mid;
                let q = self.query_text.trim();
                let title = if q.is_empty() {
                    "Nothing indexed matches the filter".to_string()
                } else {
                    format!("No results for “{q}”")
                };
                y += widgets::centered_text(ui, rect, y, &title, p.body_size + 2.0, p.text) + 8.0;
                let mut hints = Vec::new();
                if self.filter_idx != 0
                    && let Some(f) = self.config.filters.get(self.filter_idx)
                {
                    hints.push(format!("the {} filter is on", f.name));
                }
                if self.config.match_case
                    || self
                        .results
                        .as_ref()
                        .is_some_and(|r| r.query.triggers_auto_match_case)
                {
                    hints.push("Match case is on (Ctrl+I)".into());
                }
                if self.config.enable_regex {
                    hints.push("regular expressions are on (Ctrl+R)".into());
                }
                let hint = if hints.is_empty() {
                    "Check the spelling, or search in paths with Ctrl+U".to_string()
                } else {
                    let mut h = hints.join(", ");
                    if let Some(c) = h.get_mut(0..1) {
                        c.make_ascii_uppercase();
                    }
                    h
                };
                widgets::centered_text(ui, rect, y, &hint, p.body_size, p.text2);
            }
            return true;
        }
        false
    }

    fn table(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.empty_state(ui, ctx) {
            return;
        }
        let n = self.visible_len();
        let Some(results) = self.results.as_ref().map(|r| Results {
            id: r.id,
            store: r.store.clone(),
            view: r.view.clone(),
            query: r.query.clone(),
            elapsed: r.elapsed,
        }) else {
            return;
        };
        let p = self.pal;
        let cfg = &self.config;
        // responsive: secondary columns step aside on narrow windows
        let w = ui.available_width();
        let cols: Vec<(Prop, &str, f32, bool)> = vec![
            (
                Prop::Name,
                "Name",
                cfg.name_column_width.max(48) as f32,
                true,
            ),
            (
                Prop::Path,
                "Path",
                cfg.path_column_width.max(48) as f32,
                cfg.show_path_column && w >= 380.0,
            ),
            (
                Prop::FileType,
                "Type",
                cfg.type_column_width.max(48) as f32,
                cfg.show_type_column && w >= 480.0,
            ),
            (
                Prop::Extension,
                "Ext",
                cfg.extension_column_width.max(36) as f32,
                cfg.show_extension_column && w >= 440.0,
            ),
            (
                Prop::Size,
                "Size",
                cfg.size_column_width.max(48) as f32,
                cfg.show_size_column && w >= 400.0,
            ),
            (
                Prop::Mtime,
                "Modified",
                cfg.modified_column_width.max(48) as f32,
                cfg.show_modified_column && w >= 460.0,
            ),
        ];
        let cols: Vec<_> = cols.into_iter().filter(|c| c.3).collect();
        let col_props: Vec<Prop> = cols.iter().map(|c| c.0).collect();
        let row_h = p.row_height;
        let mut tb = TableBuilder::new(ui)
            .id_salt(("results_table", col_props))
            .striped(p.stripe.is_some())
            .resizable(true)
            .sense(egui::Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
        for (k, c) in cols.iter().enumerate() {
            let col = if k + 1 == cols.len() {
                Column::remainder().at_least(64.0)
            } else {
                Column::initial(c.2).at_least(48.0).clip(true)
            };
            tb = tb.column(col);
        }
        if let Some(t) = self.scroll_to.take() {
            tb = tb.scroll_to_row(t, None);
        }
        let mut action: Option<RowAction> = None;
        let mut sort_click = None;
        let mut md = MatchData::new(&results.store.db);
        let base2 = cfg.show_base_2_units;
        let show_icons = cfg.show_listview_icons;
        let tooltips = cfg.enable_list_tooltips;
        let mono = egui::FontId::monospace(p.small_size + 1.0);
        let small = egui::FontId::proportional(p.body_size - 1.0);
        let mut rendered_widths = Vec::new();
        tb.header(row_h, |mut header| {
            for c in &cols {
                header.col(|ui| {
                    let right = matches!(c.0, Prop::Size);
                    let layout = if right {
                        egui::Layout::right_to_left(egui::Align::Center)
                    } else {
                        egui::Layout::left_to_right(egui::Align::Center)
                    };
                    ui.with_layout(layout, |ui| {
                        ui.add_space(4.0);
                        let active = self.sort.0 == c.0;
                        let txt = RichText::new(c.1).size(p.body_size - 1.0).color(if active {
                            p.text
                        } else {
                            p.text2
                        });
                        let r = ui.add(egui::Button::new(txt).frame(false));
                        if r.clicked() {
                            sort_click = Some(c.0);
                        }
                        if active {
                            paint_sort_arrow(ui, &p, self.sort.1 == SortDir::Ascending);
                        }
                    });
                    let rr = ui.max_rect();
                    ui.painter()
                        .hline(rr.x_range(), rr.bottom(), Stroke::new(1.0, p.hairline));
                });
            }
        })
        .body(|body| {
            rendered_widths = body.widths().to_vec();
            body.rows(row_h, n, |mut row| {
                let i = row.index();
                let Some(id) = results.view.get(i) else {
                    return;
                };
                let selected = self.selection.contains(&id);
                row.set_selected(selected && !p.sel_marker);
                let filled = selected && !p.sel_marker;
                let e = *results.store.db.entry(id);
                for (ci, c) in cols.iter().enumerate() {
                    row.col(|ui| {
                        if selected && p.sel_marker {
                            ui.painter().rect_filled(ui.max_rect(), 0.0, p.raised);
                            if ci == 0 {
                                let r = ui.max_rect();
                                ui.painter().rect_filled(
                                    egui::Rect::from_min_size(r.min, vec2(2.0, r.height())),
                                    0.0,
                                    p.accent,
                                );
                            }
                        }
                        let fg2 = if filled { p.sel_fg } else { p.text2 };
                        match c.0 {
                            Prop::Name => {
                                ui.add_space(6.0);
                                if show_icons {
                                    let ic = if id.is_folder() {
                                        Icon::Folder
                                    } else {
                                        Icon::File
                                    };
                                    let col = if filled {
                                        p.sel_fg
                                    } else if id.is_folder() {
                                        p.text2
                                    } else {
                                        p.muted
                                    };
                                    widgets::icon(ui, ic, col);
                                    ui.add_space(2.0);
                                }
                                let job = self.name_job(&results, id, &mut md, selected);
                                ui.add(egui::Label::new(job).selectable(false));
                            }
                            Prop::Path => {
                                let s = String::from_utf8_lossy(&results.store.db.parent_path(id))
                                    .into_owned();
                                let g = widgets::elide_left(
                                    ui,
                                    &s,
                                    small.clone(),
                                    fg2,
                                    ui.available_width() - 4.0,
                                );
                                ui.add(egui::Label::new(g).selectable(false));
                            }
                            Prop::FileType => {
                                let t = fsearch_core::mime::file_type(
                                    results.store.db.display_name(id),
                                    id.is_folder(),
                                );
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(t).font(small.clone()).color(fg2),
                                    )
                                    .truncate()
                                    .selectable(false),
                                );
                            }
                            Prop::Extension => {
                                let x = results.store.db.extension(id).unwrap_or(b"");
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(String::from_utf8_lossy(x))
                                            .font(mono.clone())
                                            .color(fg2),
                                    )
                                    .truncate()
                                    .selectable(false),
                                );
                            }
                            Prop::Size => {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.add_space(8.0);
                                        let s = if id.is_folder()
                                            && e.size == 0
                                            && e.num_files + e.num_folders == 0
                                        {
                                            "—".to_string()
                                        } else {
                                            format::size(e.size, base2)
                                        };
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(s).font(mono.clone()).color(fg2),
                                            )
                                            .selectable(false),
                                        );
                                    },
                                );
                            }
                            Prop::Mtime => {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(format::date(e.mtime))
                                            .font(mono.clone())
                                            .color(fg2),
                                    )
                                    .selectable(false),
                                );
                            }
                            _ => {}
                        }
                    });
                }
                let resp = row.response();
                let resp = if tooltips {
                    resp.on_hover_text_at_pointer(
                        String::from_utf8_lossy(&results.store.db.full_path(id)).into_owned(),
                    )
                } else {
                    resp
                };
                if resp.clicked() || resp.secondary_clicked() || resp.double_clicked() {
                    let mods = ctx.input(|i| i.modifiers);
                    action = Some(RowAction::Click(
                        i,
                        mods,
                        resp.double_clicked(),
                        resp.secondary_clicked(),
                    ));
                }
                resp.context_menu(|ui| {
                    if Self::menu_item(ui, "Open", "Enter") {
                        action = Some(RowAction::Open);
                    }
                    if Self::menu_item(ui, "Open containing folder", "Ctrl+Enter") {
                        action = Some(RowAction::OpenFolder);
                    }
                    ui.separator();
                    if Self::menu_item(ui, "Copy name", "") {
                        action = Some(RowAction::Copy(0));
                    }
                    if Self::menu_item(ui, "Copy folder path", "") {
                        action = Some(RowAction::Copy(1));
                    }
                    if Self::menu_item(ui, "Copy path", "Ctrl+C") {
                        action = Some(RowAction::Copy(2));
                    }
                    ui.separator();
                    if Self::menu_item(ui, "Move to trash", "Delete") {
                        action = Some(RowAction::Trash);
                    }
                });
            });
        });
        if self.config.restore_column_config {
            let mut widths_changed = false;
            for (ci, c) in cols.iter().enumerate() {
                if ci + 1 < cols.len()
                    && let Some(&rw) = rendered_widths.get(ci)
                    && rw >= 24.0
                {
                    let wi = rw.round() as i32;
                    let slot = match c.0 {
                        Prop::Name => &mut self.config.name_column_width,
                        Prop::Path => &mut self.config.path_column_width,
                        Prop::FileType => &mut self.config.type_column_width,
                        Prop::Extension => &mut self.config.extension_column_width,
                        Prop::Size => &mut self.config.size_column_width,
                        Prop::Mtime => &mut self.config.modified_column_width,
                        _ => continue,
                    };
                    if *slot != wi {
                        *slot = wi;
                        widths_changed = true;
                    }
                }
            }
            if widths_changed && ctx.input(|i| i.pointer.any_released()) {
                self.persist_config();
            }
        }
        if let Some(prop) = sort_click {
            self.set_sort(prop);
        }
        match action {
            Some(RowAction::Open) => self.open_paths(ctx, false, false),
            Some(RowAction::OpenFolder) => self.open_paths(ctx, true, false),
            Some(RowAction::Copy(k)) => self.copy(k),
            Some(RowAction::Trash) => self.confirm_trash = Some(self.selected_paths()),
            Some(RowAction::Click(i, mods, double, secondary)) => {
                let already = self
                    .entry_at(i)
                    .is_some_and(|id| self.selection.contains(&id));
                if !(secondary && already) {
                    self.click_row(i, mods);
                }
                if double || (self.config.single_click_open && !secondary && !mods.any()) {
                    self.open_paths(ctx, false, false);
                }
            }
            None => {}
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let p = self.pal;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if p.chrome == Chrome::Headerbar || !self.config.show_menubar {
                let r = widgets::icon_button(ui, &p, Icon::Menu, "Menu");
                egui::Popup::menu(&r).show(|ui| {
                    ui.set_min_width(220.0);
                    self.menus(ui, ctx);
                });
            }
            // right-hand controls first, the search field takes the rest
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if widgets::icon_button(ui, &p, Icon::Gear, "Preferences  (Ctrl+P)").clicked() {
                    self.prefs = Some(prefs::Prefs::new(&self.config));
                }
                if widgets::icon_button(ui, &p, Icon::Refresh, "Update the index  (Ctrl+Shift+R)")
                    .clicked()
                {
                    let _ = self.backend.tx.send(Request::Rescan);
                }
                if self.config.show_filter && ui.available_width() > 360.0 {
                    let names: Vec<String> =
                        self.config.filters.iter().map(|f| f.name.clone()).collect();
                    let before = self.filter_idx;
                    egui::ComboBox::from_id_salt("filter")
                        .width(128.0)
                        .height(400.0)
                        .selected_text(names.get(self.filter_idx).cloned().unwrap_or_default())
                        .show_ui(ui, |ui| {
                            for (i, n) in names.iter().enumerate() {
                                ui.selectable_value(&mut self.filter_idx, i, n);
                            }
                        })
                        .response
                        .on_hover_text("Filter");
                    if before != self.filter_idx {
                        self.search(false);
                    }
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let w = ui.available_width().max(120.0);
                    self.search_field(ui, w);
                });
            });
        });
    }

    fn search_field(&mut self, ui: &mut egui::Ui, width: f32) {
        let p = self.pal;
        let h = if p.bevel { 24.0 } else { 32.0 };
        let (rect, _) = ui.allocate_exact_size(vec2(width, h), egui::Sense::hover());
        let rad = p.radius as f32;
        if p.bevel {
            ui.painter().rect_filled(rect, 0.0, Color32::WHITE);
            widgets::sunken(ui.painter(), rect);
        } else {
            ui.painter().rect_filled(rect, rad, p.raised);
            let stroke = if self.search_has_focus {
                Stroke::new(1.5, p.accent)
            } else {
                Stroke::new(1.0, p.hairline)
            };
            ui.painter()
                .rect_stroke(rect, rad, stroke, egui::StrokeKind::Inside);
        }
        let inner = rect.shrink2(vec2(10.0, 0.0));
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                widgets::icon(ui, Icon::Search, p.muted);
                let toggles_w = 3.0 * 30.0 + 4.0;
                let te = egui::TextEdit::singleline(&mut self.query_text)
                    .frame(egui::Frame::NONE)
                    .hint_text(RichText::new("Search").color(p.muted))
                    .text_color(p.text)
                    .font(egui::FontId::proportional(p.body_size + 1.0))
                    .desired_width((ui.available_width() - toggles_w).max(40.0))
                    .align(egui::Align2::LEFT_CENTER)
                    .id_salt("search");
                let resp = ui.add(te);
                if self.focus_search {
                    resp.request_focus();
                    self.focus_search = false;
                }
                self.search_has_focus = resp.has_focus();
                if resp.changed() && self.config.search_as_you_type {
                    self.pending_search_at = Some(Instant::now());
                }
                if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    self.search(!self.config.search_as_you_type);
                    if self.visible_len() > 0 {
                        // Enter in the field moves to the first result
                        self.click_row(0, Modifiers::NONE);
                    }
                }
                if resp.has_focus() && ui.input(|i| i.key_pressed(Key::ArrowDown)) {
                    resp.surrender_focus();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let auto_path = self
                        .results
                        .as_ref()
                        .is_some_and(|r| r.query.triggers_auto_match_path);
                    let auto_case = self
                        .results
                        .as_ref()
                        .is_some_and(|r| r.query.triggers_auto_match_case);
                    let mut path = self.config.search_in_path;
                    let tip_path = if auto_path && !path {
                        "Search in path  (Ctrl+U)\nOn for this query: it contains /"
                    } else {
                        "Search in path  (Ctrl+U)"
                    };
                    if widgets::keycap_toggle(ui, &p, "/", &mut path, tip_path).changed() {
                        self.config.search_in_path = path;
                    }
                    let mut rx = self.config.enable_regex;
                    if widgets::keycap_toggle(
                        ui,
                        &p,
                        ".*",
                        &mut rx,
                        "Regular expressions  (Ctrl+R)",
                    )
                    .changed()
                    {
                        self.config.enable_regex = rx;
                    }
                    let mut case = self.config.match_case;
                    let tip_case = if auto_case && !case {
                        "Match case  (Ctrl+I)\nOn for this query: it has capitals"
                    } else {
                        "Match case  (Ctrl+I)"
                    };
                    if widgets::keycap_toggle(ui, &p, "Aa", &mut case, tip_case).changed() {
                        self.config.match_case = case;
                    }
                });
            },
        );
    }

    fn statusbar(&mut self, ui: &mut egui::Ui) {
        let p = self.pal;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let n = self.visible_len();
            let sel = self.selection.len();
            let mono = |s: String, c: Color32| {
                RichText::new(s)
                    .font(egui::FontId::monospace(p.small_size + 1.0))
                    .color(c)
            };
            ui.label(mono(format::count(n), p.text));
            ui.label(
                RichText::new(if n == 1 { "result" } else { "results" })
                    .color(p.text2)
                    .size(p.small_size + 1.0),
            );
            if sel > 0 {
                ui.label(RichText::new("·").color(p.muted));
                ui.label(mono(format::count(sel), p.text));
                ui.label(
                    RichText::new("selected")
                        .color(p.text2)
                        .size(p.small_size + 1.0),
                );
            }
            if let Some(r) = &self.results
                && self.task.is_none()
            {
                ui.label(RichText::new("·").color(p.muted));
                ui.label(mono(
                    format!("{:.1} ms", r.elapsed.as_secs_f64() * 1000.0),
                    p.muted,
                ));
            }
            if let Some(t) = self.task {
                ui.label(RichText::new("·").color(p.muted));
                ui.label(RichText::new(t).color(p.text2).size(p.small_size + 1.0));
            }
            ui.with_layout(
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| match self.db {
                    DbState::Loading => {
                        ui.label(
                            RichText::new("Loading the index")
                                .color(p.text2)
                                .size(p.small_size + 1.0),
                        );
                        ui.add(egui::Spinner::new().size(12.0).color(p.muted));
                    }
                    DbState::Scanning => {
                        let k = self
                            .backend
                            .scan_progress
                            .load(std::sync::atomic::Ordering::Relaxed);
                        ui.label(mono(format::count(k as usize), p.text2));
                        ui.label(
                            RichText::new("Indexing folders")
                                .color(p.text2)
                                .size(p.small_size + 1.0),
                        );
                        ui.add(egui::Spinner::new().size(12.0).color(p.muted));
                        ui.ctx().request_repaint_after(Duration::from_millis(120));
                    }
                    DbState::Ready(total) => {
                        ui.label(
                            RichText::new("indexed")
                                .color(p.muted)
                                .size(p.small_size + 1.0),
                        );
                        ui.label(mono(format::count(total), p.text2));
                    }
                },
            );
        });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let p = self.pal;
        if let Some(pr) = &mut self.prefs {
            match pr.show(ctx, &p) {
                prefs::Outcome::Open => {}
                prefs::Outcome::Cancel => self.prefs = None,
                prefs::Outcome::Preview(look) => {
                    if look != self.look {
                        self.show_look(ctx, look);
                    }
                }
                prefs::Outcome::Apply(c) => {
                    self.prefs = None;
                    self.apply_config(ctx, *c);
                }
            }
            if self.prefs.is_none() {
                // cancelling restores the saved look
                let saved = Look::from_id(&self.config.look);
                if saved != self.look {
                    self.show_look(ctx, saved);
                }
            }
        }
        if self.about {
            let m = egui::Modal::new(egui::Id::unique("about")).show(ctx, |ui| {
                ui.set_width(340.0);
                ui.label(RichText::new("FSearch").size(p.body_size + 6.0).color(p.text));
                ui.label(RichText::new(format!("Version {VERSION}")).font(egui::FontId::monospace(p.small_size + 1.0)).color(p.muted));
                ui.add_space(8.0);
                ui.label(RichText::new("A fast file search utility. Rust port of FSearch by Christian Boxdörfer, GPL-2.0-or-later.").color(p.text2));
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if ui.button("Website").clicked() {
                        let _ = open::that_detached("https://github.com/cboxdoerfer/fsearch");
                    }
                    if ui.button("Close").clicked() {
                        ui.close();
                    }
                });
            });
            if m.should_close() {
                self.about = false;
            }
        }
        if let Some((n, folders, keyboard)) = self.confirm_open {
            let mut go = false;
            let m = egui::Modal::new(egui::Id::unique("open-many")).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(
                    RichText::new(format!("Open {} items?", format::count(n)))
                        .size(p.body_size + 2.0)
                        .color(p.text),
                );
                ui.add_space(4.0);
                ui.label(RichText::new("Each one starts its application.").color(p.text2));
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    let primary = egui::Button::new(RichText::new("Open all").color(p.on_accent))
                        .fill(p.accent);
                    if ui.add(primary).clicked() {
                        go = true;
                        ui.close();
                    }
                    if ui.button("Cancel").clicked() {
                        ui.close();
                    }
                });
            });
            if go {
                let paths = self.selected_paths();
                self.open_confirmed(ctx, paths, folders, keyboard);
            }
            if m.should_close() || go {
                self.confirm_open = None;
            }
        }
        if let Some(paths) = self.confirm_trash.clone() {
            let m = egui::Modal::new(egui::Id::unique("trash")).show(ctx, |ui| {
                ui.set_width(380.0);
                let what = if paths.len() == 1 {
                    paths[0]
                        .file_name()
                        .map(|n| format!("“{}”", n.to_string_lossy()))
                        .unwrap_or_else(|| "1 item".into())
                } else {
                    format!("{} items", format::count(paths.len()))
                };
                ui.label(
                    RichText::new(format!("Move {what} to the trash?"))
                        .size(p.body_size + 2.0)
                        .color(p.text),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("You can restore them from the trash later.").color(p.text2),
                );
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    let primary =
                        egui::Button::new(RichText::new("Move to trash").color(p.on_accent))
                            .fill(p.accent);
                    if ui.add(primary).clicked() {
                        if let Err(e) = trash::delete_all(&paths) {
                            self.error = Some(format!("Could not move to the trash: {e}"));
                        }
                        self.selection.clear();
                        let _ = self.backend.tx.send(Request::Rescan);
                        ui.close();
                    }
                    if ui.button("Cancel").clicked() {
                        ui.close();
                    }
                });
            });
            if m.should_close() {
                self.confirm_trash = None;
            }
        }
        if let Some(e) = self.error.clone() {
            let neg = if p.dark {
                Color32::from_rgb(0xe5, 0x48, 0x4d)
            } else {
                Color32::from_rgb(0xdc, 0x26, 0x26)
            };
            let m = egui::Modal::new(egui::Id::unique("error")).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(
                    RichText::new("Something went wrong")
                        .size(p.body_size + 2.0)
                        .color(neg),
                );
                ui.add_space(4.0);
                ui.label(RichText::new(e).color(p.text2));
                ui.add_space(16.0);
                if ui.button("OK").clicked() {
                    ui.close();
                }
            });
            if m.should_close() {
                self.error = None;
            }
        }
    }
}

fn paint_sort_arrow(ui: &mut egui::Ui, p: &Palette, ascending: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), egui::Sense::hover());
    let c = rect.center();
    let s = 3.5;
    let pts = if ascending {
        vec![
            egui::pos2(c.x - s, c.y + s * 0.6),
            egui::pos2(c.x + s, c.y + s * 0.6),
            egui::pos2(c.x, c.y - s * 0.6),
        ]
    } else {
        vec![
            egui::pos2(c.x - s, c.y - s * 0.6),
            egui::pos2(c.x + s, c.y - s * 0.6),
            egui::pos2(c.x, c.y + s * 0.6),
        ]
    };
    ui.painter()
        .add(egui::Shape::convex_polygon(pts, p.text2, Stroke::NONE));
}

impl App {
    fn screenshot_hook(&mut self, ctx: &egui::Context) {
        let Some((path, frames)) = self.screenshot.as_mut() else {
            return;
        };
        let ready = (self.results.is_some() || self.config.includes.is_empty())
            && matches!(self.db, DbState::Ready(_))
            && self.task.is_none();
        if ready && self.fonts_with_fallbacks {
            *frames += 1;
            // wait for open animations (modals fade in) before capturing
            if *frames == 2 {
                self.screenshot_ready_at = Some(Instant::now());
            }
            if self
                .screenshot_ready_at
                .is_some_and(|t| t.elapsed() > Duration::from_millis(900))
            {
                self.screenshot_ready_at = None;
                *frames = 1000;
                let path = path.clone();
                let ctx2 = ctx.clone();
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                    egui::ScreenshotCallback::new(move |img| {
                        if let Ok(f) = std::fs::File::create(&path) {
                            let mut enc = png::Encoder::new(
                                std::io::BufWriter::new(f),
                                img.size[0] as u32,
                                img.size[1] as u32,
                            );
                            enc.set_color(png::ColorType::Rgba);
                            enc.set_depth(png::BitDepth::Eight);
                            if let Ok(mut w) = enc.write_header() {
                                let _ = w.write_image_data(img.as_raw());
                            }
                        }
                        ctx2.send_viewport_cmd(egui::ViewportCommand::Close);
                        ctx2.request_repaint();
                    }),
                ));
            }
        }
        ctx.request_repaint();
    }

    /// Test hook for screenshots: `FSEARCH_SCREENSHOT_UI=prefs|about|trash|menu` opens that UI.
    fn screenshot_ui(&mut self) {
        if self.screenshot.as_ref().is_some_and(|s| s.1 == 1)
            && let Ok(w) = std::env::var("FSEARCH_SCREENSHOT_UI")
        {
            match w.as_str() {
                "prefs" => self.prefs = Some(prefs::Prefs::new(&self.config)),
                "prefs-db" => {
                    let mut p = prefs::Prefs::new(&self.config);
                    p.show_database();
                    self.prefs = Some(p);
                }
                "prefs-keys" => {
                    let mut p = prefs::Prefs::new(&self.config);
                    p.show_shortcuts();
                    self.prefs = Some(p);
                }
                "about" => self.about = true,
                "select" => {
                    for i in [1usize, 2, 4] {
                        if let Some(id) = self.entry_at(i) {
                            self.selection.insert(id);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn persist_config(&self) {
        if Config::unreadable() {
            return;
        }
        let mut c = Config::load_or_default();
        c.window_width = self.config.window_width;
        c.window_height = self.config.window_height;
        c.sort_by = self.config.sort_by.clone();
        c.sort_ascending = self.config.sort_ascending;
        c.match_case = self.config.match_case;
        c.enable_regex = self.config.enable_regex;
        c.search_in_path = self.config.search_in_path;
        c.auto_match_case = self.config.auto_match_case;
        c.auto_search_in_path = self.config.auto_search_in_path;
        c.show_statusbar = self.config.show_statusbar;
        c.show_filter = self.config.show_filter;
        c.show_menubar = self.config.show_menubar;
        c.show_path_column = self.config.show_path_column;
        c.show_type_column = self.config.show_type_column;
        c.show_extension_column = self.config.show_extension_column;
        c.show_size_column = self.config.show_size_column;
        c.show_modified_column = self.config.show_modified_column;
        c.name_column_width = self.config.name_column_width;
        c.path_column_width = self.config.path_column_width;
        c.type_column_width = self.config.type_column_width;
        c.extension_column_width = self.config.extension_column_width;
        c.size_column_width = self.config.size_column_width;
        c.modified_column_width = self.config.modified_column_width;
        c.restore_column_config = self.config.restore_column_config;
        c.restore_window_size = self.config.restore_window_size;
        c.restore_sort_order = self.config.restore_sort_order;
        c.look = self.look.id().into();
        let _ = c.save();
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        self.screenshot_ui();
        if self.prefs.is_none()
            && !self.about
            && self.confirm_trash.is_none()
            && self.confirm_open.is_none()
        {
            self.shortcuts(&ctx);
        }
        let flags_before = self.flags();
        let p = self.pal;
        let bar_frame = egui::Frame::new()
            .fill(p.bar)
            .inner_margin(egui::Margin::symmetric(12, 8));

        egui::Panel::top("header")
            .frame(bar_frame)
            .show_separator_line(false)
            .show(ui, |ui| {
                if p.chrome == Chrome::Menubar && self.config.show_menubar {
                    egui::MenuBar::new().ui(ui, |ui| self.menus(ui, &ctx));
                    ui.add_space(6.0);
                }
                self.toolbar(ui, &ctx);
            });
        let line = |ui: &mut egui::Ui, top: bool| {
            let r = ui.max_rect();
            let y = if top { r.top() } else { r.bottom() };
            ui.painter().hline(
                r.x_range(),
                y,
                Stroke::new(
                    1.0,
                    if p.bevel {
                        Color32::from_gray(0x80)
                    } else {
                        p.hairline
                    },
                ),
            );
        };
        if self.config.show_statusbar {
            let sf = egui::Frame::new()
                .fill(p.bar)
                .inner_margin(egui::Margin::symmetric(12, 6));
            egui::Panel::bottom("status")
                .frame(sf)
                .show_separator_line(false)
                .show(ui, |ui| {
                    line(ui, true);
                    self.statusbar(ui);
                });
        }
        let cf = egui::Frame::new().fill(p.page).inner_margin(egui::Margin {
            left: 4,
            right: 4,
            top: 0,
            bottom: 0,
        });
        egui::CentralPanel::default().frame(cf).show(ui, |ui| {
            line(ui, true);
            if p.bevel {
                widgets::sunken(ui.painter(), ui.max_rect());
            }
            let rows = (ui.available_height() / p.row_height) as usize;
            if !ctx.memory(|m| m.focused().is_some()) && self.prefs.is_none() {
                self.list_keys(&ctx, rows.saturating_sub(2));
            }
            self.table(ui, &ctx);
        });

        if let Some(t) = self.pending_search_at {
            if t.elapsed() >= Duration::from_millis(30) {
                self.pending_search_at = None;
                self.search(false);
            } else {
                ctx.request_repaint_after(Duration::from_millis(30));
            }
        }
        if self.flags() != flags_before {
            self.search(false);
        }
        self.dialogs(&ctx);
        self.screenshot_hook(&ctx);
        if self.config.restore_window_size
            && let Some(r) = ctx.input(|i| i.viewport().inner_rect)
        {
            self.config.window_width = r.width() as i32;
            self.config.window_height = r.height() as i32;
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // persist UI state the way the original does on close (never over an unreadable file)
        self.persist_config();
        let _ = self.backend.tx.send(Request::SaveAndQuit);
        ipc::cleanup();
    }
}

/// The FSearch icon (rendered from `data/io.github.cboxdoerfer.FSearch.svg`).
fn window_icon() -> egui::IconData {
    let bytes: &[u8] = include_bytes!("../../assets/fsearch-256.png");
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let Ok(mut reader) = decoder.read_info() else {
        return egui::IconData::default();
    };
    let Some(size) = reader.output_buffer_size() else {
        return egui::IconData::default();
    };
    let mut buf = vec![0u8; size];
    let Ok(info) = reader.next_frame(&mut buf) else {
        return egui::IconData::default();
    };
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        _ => return egui::IconData::default(),
    };
    egui::IconData {
        rgba,
        width: info.width,
        height: info.height,
    }
}

/// Start the GUI. Returns the process exit code.
pub fn run(opts: StartOptions) -> u8 {
    let config = Config::load_or_default();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("FSearch")
        .with_icon(window_icon())
        .with_app_id("io.github.cboxdoerfer.FSearch")
        .with_min_inner_size([360.0, 240.0]);
    viewport = if config.restore_window_size {
        viewport.with_inner_size([
            config.window_width.clamp(360, 8192) as f32,
            config.window_height.clamp(240, 8192) as f32,
        ])
    } else {
        viewport.with_inner_size([850.0, 600.0])
    };
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let res = eframe::run_native(
        "FSearch",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, config, opts)))),
    );
    match res {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("fsearch: {e}");
            1
        }
    }
}
