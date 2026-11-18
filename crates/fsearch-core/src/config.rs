//! Port of `src/fsearch_config.c`: `~/.config/fsearch/fsearch.conf`, a GKeyFile, read and
//! written in the same layout so the C and Rust builds share one config.

use crate::filter::{Filter, default_filters};
use crate::query::flags;
use crate::rules::{Exclude, ExcludeManager, ExcludeType, Include, Scope, Target, add_include};
use std::path::PathBuf;

/// A minimal GKeyFile: ordered groups of ordered keys, values kept escaped.
#[derive(Default, Debug, Clone)]
pub struct KeyFile {
    groups: Vec<(String, Vec<(String, String)>)>,
}

impl KeyFile {
    pub fn parse(text: &str) -> KeyFile {
        let mut kf = KeyFile::default();
        for raw in text.lines() {
            let line = raw.trim_start();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                if let Some(end) = line.find(']') {
                    let name = line[1..end].to_string();
                    if !kf.groups.iter().any(|(g, _)| *g == name) {
                        kf.groups.push((name, Vec::new()));
                    }
                }
                continue;
            }
            let Some(eq) = line.find('=') else { continue };
            let key = line[..eq].trim_end().to_string();
            let value = line[eq + 1..].trim_start().to_string();
            if let Some((_, keys)) = kf.groups.last_mut() {
                if let Some(slot) = keys.iter_mut().find(|(k, _)| *k == key) {
                    slot.1 = value;
                } else {
                    keys.push((key, value));
                }
            }
        }
        kf
    }

    pub fn has_group(&self, g: &str) -> bool {
        self.groups.iter().any(|(n, _)| n == g)
    }

    pub fn has_key(&self, g: &str, k: &str) -> bool {
        self.raw(g, k).is_some()
    }

    fn raw(&self, g: &str, k: &str) -> Option<&str> {
        self.groups
            .iter()
            .find(|(n, _)| n == g)?
            .1
            .iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.as_str())
    }

    pub fn get_string(&self, g: &str, k: &str) -> Option<String> {
        let v = self.raw(g, k)?;
        let mut out = String::with_capacity(v.len());
        let mut it = v.chars();
        while let Some(c) = it.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match it.next() {
                Some('s') => out.push(' '),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(o) => {
                    out.push('\\');
                    out.push(o);
                }
                None => return None,
            }
        }
        Some(out)
    }

    pub fn get_bool(&self, g: &str, k: &str) -> Option<bool> {
        match self.raw(g, k)?.trim_end() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        }
    }

    pub fn get_int(&self, g: &str, k: &str) -> Option<i64> {
        self.raw(g, k)?.trim().parse().ok()
    }

    fn set_raw(&mut self, g: &str, k: &str, v: String) {
        if !self.has_group(g) {
            self.groups.push((g.to_string(), Vec::new()));
        }
        let keys = &mut self
            .groups
            .iter_mut()
            .find(|(n, _)| n == g)
            .map(|x| &mut x.1);
        if let Some(keys) = keys {
            if let Some(slot) = keys.iter_mut().find(|(key, _)| key == k) {
                slot.1 = v;
            } else {
                keys.push((k.to_string(), v));
            }
        }
    }

    pub fn set_string(&mut self, g: &str, k: &str, v: &str) {
        let mut out = String::with_capacity(v.len());
        let mut leading = true;
        for c in v.chars() {
            match c {
                ' ' if leading => out.push_str("\\s"),
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                '\r' => out.push_str("\\r"),
                '\\' => out.push_str("\\\\"),
                c => out.push(c),
            }
            if c != ' ' {
                leading = false;
            }
        }
        self.set_raw(g, k, out);
    }

    pub fn set_bool(&mut self, g: &str, k: &str, v: bool) {
        self.set_raw(g, k, if v { "true" } else { "false" }.into());
    }

    pub fn set_int(&mut self, g: &str, k: &str, v: i64) {
        self.set_raw(g, k, v.to_string());
    }

    /// `g_key_file_to_data`: groups separated by a blank line.
    pub fn to_data(&self) -> String {
        let mut out = String::new();
        for (i, (g, keys)) in self.groups.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push('[');
            out.push_str(g);
            out.push_str("]\n");
            for (k, v) in keys {
                out.push_str(k);
                out.push('=');
                out.push_str(v);
                out.push('\n');
            }
        }
        out
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    // Search
    pub hide_results_on_empty_search: bool,
    pub search_in_path: bool,
    pub enable_regex: bool,
    pub match_case: bool,
    pub auto_search_in_path: bool,
    pub auto_match_case: bool,
    pub search_as_you_type: bool,
    // Applications
    pub folder_open_cmd: Option<String>,
    // Window
    pub restore_window_size: bool,
    pub window_width: i32,
    pub window_height: i32,
    // Interface
    pub show_base_2_units: bool,
    pub highlight_search_terms: bool,
    pub single_click_open: bool,
    pub launch_desktop_files: bool,
    pub enable_dark_theme: bool,
    pub enable_list_tooltips: bool,
    pub restore_column_config: bool,
    pub restore_sort_order: bool,
    pub double_click_path: bool,
    pub action_after_file_open_keyboard: bool,
    pub action_after_file_open_mouse: bool,
    pub action_after_file_open: i32,
    pub exit_on_escape: bool,
    pub show_indexing_status: bool,
    pub show_menubar: bool,
    pub show_statusbar: bool,
    pub show_filter: bool,
    pub show_search_button: bool,
    pub show_listview_icons: bool,
    pub show_path_column: bool,
    pub show_type_column: bool,
    pub show_extension_column: bool,
    pub show_size_column: bool,
    pub show_modified_column: bool,
    pub sort_ascending: bool,
    pub sort_by: Option<String>,
    pub name_column_width: i32,
    pub path_column_width: i32,
    pub type_column_width: i32,
    pub extension_column_width: i32,
    pub size_column_width: i32,
    pub modified_column_width: i32,
    pub name_column_pos: i32,
    pub path_column_pos: i32,
    pub type_column_pos: i32,
    pub size_column_pos: i32,
    pub modified_column_pos: i32,
    // Dialogs
    pub show_dialog_failed_opening: bool,
    /// Rust port only: the window look (`gui::theme::Look` id). The C build ignores the key.
    pub look: String,
    pub filters: Vec<Filter>,
    pub includes: Vec<Include>,
    pub excludes: ExcludeManager,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            hide_results_on_empty_search: false,
            search_in_path: false,
            enable_regex: false,
            match_case: false,
            auto_search_in_path: true,
            auto_match_case: true,
            search_as_you_type: true,
            folder_open_cmd: None,
            restore_window_size: true,
            window_width: 850,
            window_height: 600,
            show_base_2_units: false,
            highlight_search_terms: true,
            single_click_open: false,
            launch_desktop_files: true,
            enable_dark_theme: false,
            enable_list_tooltips: true,
            restore_column_config: true,
            restore_sort_order: true,
            double_click_path: false,
            action_after_file_open_keyboard: false,
            action_after_file_open_mouse: false,
            action_after_file_open: 0,
            exit_on_escape: false,
            show_indexing_status: true,
            show_menubar: true,
            show_statusbar: true,
            show_filter: true,
            show_search_button: false,
            show_listview_icons: true,
            show_path_column: true,
            show_type_column: false,
            show_extension_column: true,
            show_size_column: true,
            show_modified_column: true,
            sort_ascending: true,
            sort_by: Some("Name".into()),
            name_column_width: 250,
            path_column_width: 250,
            type_column_width: 100,
            extension_column_width: 100,
            size_column_width: 75,
            modified_column_width: 75,
            name_column_pos: 0,
            path_column_pos: 1,
            type_column_pos: 2,
            size_column_pos: 3,
            modified_column_pos: 4,
            show_dialog_failed_opening: true,
            look: "modern-dark".into(),
            filters: default_filters(),
            includes: Vec::new(),
            excludes: ExcludeManager::with_defaults(),
        }
    }
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(fallback),
    }
}

pub fn config_dir() -> PathBuf {
    #[cfg(windows)]
    if let Some(d) = std::env::var_os("APPDATA").filter(|v| !v.is_empty()) {
        return PathBuf::from(d).join("fsearch");
    }
    xdg("XDG_CONFIG_HOME", ".config").join("fsearch")
}

pub fn config_path() -> PathBuf {
    config_dir().join("fsearch.conf")
}

/// `g_get_user_data_dir()/fsearch/fsearch.db`.
pub fn database_path() -> PathBuf {
    #[cfg(windows)]
    if let Some(d) = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
        return PathBuf::from(d).join("fsearch").join("fsearch.db");
    }
    xdg("XDG_DATA_HOME", ".local/share")
        .join("fsearch")
        .join("fsearch.db")
}

macro_rules! bool_keys {
    ($m:ident) => {
        $m!(
            "Interface",
            show_base_2_units,
            highlight_search_terms,
            single_click_open,
            launch_desktop_files,
            enable_dark_theme,
            enable_list_tooltips,
            restore_column_config,
            restore_sort_order,
            double_click_path,
            action_after_file_open_keyboard,
            action_after_file_open_mouse
        );
    };
}

impl Config {
    /// The query flags the search uses (`get_query_flags` in `fsearch_window.c`).
    pub fn query_flags(&self) -> u32 {
        let mut f = 0;
        if self.match_case {
            f |= flags::MATCH_CASE;
        }
        if self.auto_match_case {
            f |= flags::AUTO_MATCH_CASE;
        }
        if self.enable_regex {
            f |= flags::REGEX;
        }
        if self.search_in_path {
            f |= flags::SEARCH_IN_PATH;
        }
        if self.auto_search_in_path {
            f |= flags::AUTO_SEARCH_IN_PATH;
        }
        f
    }

    /// `config_load`: defaults for anything missing; `None` when the file cannot be read.
    pub fn load() -> Option<Config> {
        // GKeyFile is UTF-8, but a stray byte must not make the whole file unreadable
        let bytes = std::fs::read(config_path()).ok()?;
        Some(Config::from_keyfile(&KeyFile::parse(
            &String::from_utf8_lossy(&bytes),
        )))
    }

    /// True when a config file exists but cannot be read; saving would then replace the user's
    /// settings with defaults, so callers that only persist UI state skip it.
    pub fn unreadable() -> bool {
        let p = config_path();
        p.exists() && std::fs::read(&p).is_err()
    }

    pub fn load_or_default() -> Config {
        Config::load().unwrap_or_default()
    }

    pub fn from_keyfile(kf: &KeyFile) -> Config {
        let mut c = Config::default();
        let b = |g: &str, k: &str, d: bool| kf.get_bool(g, k).unwrap_or(d);
        let i = |g: &str, k: &str, d: i32| kf.get_int(g, k).map(|v| v as i32).unwrap_or(d);
        macro_rules! load_bools {
            ($g:expr, $($f:ident),*) => { $( c.$f = b($g, stringify!($f), c.$f); )* };
        }
        bool_keys!(load_bools);
        c.action_after_file_open = i(
            "Interface",
            "action_after_file_open",
            c.action_after_file_open,
        );
        load_bools!(
            "Interface",
            exit_on_escape,
            show_indexing_status,
            show_menubar,
            show_statusbar,
            show_filter,
            show_search_button,
            show_listview_icons,
            show_path_column,
            show_type_column,
            show_extension_column,
            show_size_column,
            show_modified_column,
            sort_ascending
        );
        c.sort_by = kf.get_string("Interface", "sort_by").or(c.sort_by);
        macro_rules! load_ints {
            ($g:expr, $($f:ident),*) => { $( c.$f = i($g, stringify!($f), c.$f); )* };
        }
        load_ints!(
            "Interface",
            name_column_width,
            path_column_width,
            type_column_width,
            extension_column_width,
            size_column_width,
            modified_column_width,
            name_column_pos,
            path_column_pos,
            type_column_pos,
            size_column_pos,
            modified_column_pos
        );
        c.show_dialog_failed_opening = b("Dialogs", "show_dialog_failed_opening", true);
        if let Some(l) = kf.get_string("Interface", "look") {
            c.look = l;
        }
        c.folder_open_cmd = kf.get_string("Applications", "folder_open_cmd");
        load_bools!("Interface", restore_window_size);
        load_ints!("Interface", window_width, window_height);
        if !c.restore_column_config {
            c.show_listview_icons = true;
            c.show_path_column = true;
            c.show_type_column = false;
            c.show_extension_column = true;
            c.show_size_column = true;
            c.show_modified_column = true;
            c.name_column_width = 250;
            c.path_column_width = 250;
            c.type_column_width = 100;
            c.extension_column_width = 100;
            c.size_column_width = 75;
            c.modified_column_width = 75;
        }
        load_bools!(
            "Search",
            hide_results_on_empty_search,
            search_in_path,
            enable_regex,
            match_case,
            auto_search_in_path,
            auto_match_case,
            search_as_you_type
        );
        c.includes = if kf.has_key("Database", "location_1") {
            legacy_includes(kf)
        } else {
            load_includes(kf)
        };
        c.excludes = if kf.has_key("Database", "exclude_location_1") {
            legacy_excludes(kf)
        } else {
            load_excludes(kf)
        };
        c.filters = load_filters(kf);
        c
    }

    pub fn to_keyfile(&self) -> KeyFile {
        let mut kf = KeyFile::default();
        let g = "Interface";
        macro_rules! save_bools {
            ($g:expr, $($f:ident),*) => { $( kf.set_bool($g, stringify!($f), self.$f); )* };
        }
        macro_rules! save_ints {
            ($g:expr, $($f:ident),*) => { $( kf.set_int($g, stringify!($f), self.$f as i64); )* };
        }
        bool_keys!(save_bools);
        kf.set_int(
            g,
            "action_after_file_open",
            self.action_after_file_open as i64,
        );
        save_bools!(
            g,
            exit_on_escape,
            show_indexing_status,
            show_menubar,
            show_statusbar,
            show_filter,
            show_search_button,
            show_listview_icons,
            show_path_column,
            show_type_column,
            show_extension_column,
            show_size_column,
            show_modified_column,
            sort_ascending
        );
        if let Some(s) = &self.sort_by {
            kf.set_string(g, "sort_by", s);
        }
        save_ints!(
            g,
            name_column_width,
            path_column_width,
            type_column_width,
            extension_column_width,
            size_column_width,
            modified_column_width,
            name_column_pos,
            path_column_pos,
            type_column_pos,
            size_column_pos,
            modified_column_pos
        );
        save_bools!(g, restore_window_size);
        save_ints!(g, window_width, window_height);
        if self.look != "modern-dark" {
            kf.set_string(g, "look", &self.look);
        }
        kf.set_bool(
            "Dialogs",
            "show_dialog_failed_opening",
            self.show_dialog_failed_opening,
        );
        if let Some(cmd) = &self.folder_open_cmd {
            kf.set_string("Applications", "folder_open_cmd", cmd);
        }
        save_bools!(
            "Search",
            hide_results_on_empty_search,
            search_in_path,
            enable_regex,
            match_case,
            auto_search_in_path,
            auto_match_case,
            search_as_you_type
        );
        for (i, f) in self.filters.iter().enumerate() {
            let k = |s: &str| format!("filter_{}_{}", i + 1, s);
            kf.set_string("Filters", &k("name"), &f.name);
            kf.set_string("Filters", &k("query"), &f.query);
            kf.set_string("Filters", &k("macro"), &f.macro_name);
            kf.set_bool(
                "Filters",
                &k("match_case"),
                f.flags & flags::MATCH_CASE != 0,
            );
            kf.set_bool(
                "Filters",
                &k("search_in_path"),
                f.flags & flags::SEARCH_IN_PATH != 0,
            );
            kf.set_bool("Filters", &k("enable_regex"), f.flags & flags::REGEX != 0);
        }
        for (i, inc) in self.includes.iter().enumerate() {
            let k = |s: &str| format!("folder_{}_{}", i + 1, s);
            kf.set_bool("Database", &k("active"), inc.active);
            kf.set_string("Database", &k("path"), &inc.path);
            kf.set_bool("Database", &k("one_file_system"), inc.one_file_system);
            kf.set_bool("Database", &k("monitor"), inc.monitored);
            kf.set_bool("Database", &k("scan_after_launch"), inc.scan_after_launch);
            // stored through an `int` in the original
            kf.set_int(
                "Database",
                &k("rescan_after"),
                inc.rescan_after as i32 as i64,
            );
        }
        for (i, e) in self.excludes.excludes.iter().enumerate() {
            let k = |s: &str| format!("exclude_{}_{}", i + 1, s);
            kf.set_bool("Database", &k("active"), e.active);
            kf.set_string("Database", &k("type"), e.kind.as_str());
            kf.set_string("Database", &k("pattern"), &e.pattern);
            kf.set_string("Database", &k("match_scope"), e.scope.as_str());
            kf.set_string("Database", &k("target"), e.target.as_str());
        }
        kf.set_bool(
            "Database",
            "exclude_hidden_files_and_folders",
            self.excludes.exclude_hidden,
        );
        kf
    }

    /// `config_save`.
    pub fn save(&self) -> std::io::Result<()> {
        let dir = config_dir();
        std::fs::create_dir_all(&dir)?;
        // write a temporary file and rename it: a crash never leaves a truncated config
        let path = config_path();
        let tmp = dir.join(format!(".fsearch.conf.{}.tmp", std::process::id()));
        std::fs::write(&tmp, self.to_keyfile().to_data())?;
        std::fs::rename(&tmp, &path).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
    }
}

fn load_filters(kf: &KeyFile) -> Vec<Filter> {
    if !kf.has_group("Filters") {
        return default_filters();
    }
    let mut v = Vec::new();
    for i in 1.. {
        let k = |s: &str| format!("filter_{i}_{s}");
        let Some(name) = kf.get_string("Filters", &k("name")) else {
            break;
        };
        if name.trim().is_empty() {
            break;
        }
        let mut fl = 0;
        if kf.get_bool("Filters", &k("match_case")).unwrap_or(false) {
            fl |= flags::MATCH_CASE;
        }
        if kf
            .get_bool("Filters", &k("search_in_path"))
            .unwrap_or(false)
        {
            fl |= flags::SEARCH_IN_PATH;
        }
        if kf.get_bool("Filters", &k("enable_regex")).unwrap_or(false) {
            fl |= flags::REGEX;
        }
        let f = Filter::new(
            &name,
            kf.get_string("Filters", &k("macro")).as_deref(),
            kf.get_string("Filters", &k("query")).as_deref(),
            fl,
        );
        // fsearch_filter_manager_append_filter keeps names unique
        let mut f = f;
        let base = f.name.clone();
        let mut n = 1;
        while v.iter().any(|x: &Filter| x.name == f.name) {
            f.name = format!("{base} ({n})");
            n += 1;
        }
        v.push(f);
    }
    v
}

fn load_includes(kf: &KeyFile) -> Vec<Include> {
    if !kf.has_group("Database") {
        return Vec::new();
    }
    let mut v = Vec::new();
    for i in 1.. {
        let k = |s: &str| format!("folder_{i}_{s}");
        let Some(path) = kf.get_string("Database", &k("path")) else {
            break;
        };
        if path.trim().is_empty() {
            break;
        }
        let b = |s: &str| kf.get_bool("Database", &k(s)).unwrap_or(false);
        // read through an `int` into a zeroed int64 in the original
        let rescan = kf
            .get_int("Database", &k("rescan_after"))
            .map(|r| r as i32 as u32 as i64)
            .unwrap_or(0);
        add_include(
            &mut v,
            Include::new(
                &path,
                b("active"),
                b("one_file_system"),
                b("monitor"),
                b("scan_after_launch"),
                rescan,
            ),
        );
    }
    v
}

fn load_excludes(kf: &KeyFile) -> ExcludeManager {
    if !kf.has_group("Database") {
        return ExcludeManager::with_defaults();
    }
    let mut m = ExcludeManager::default();
    for i in 1.. {
        let k = |s: &str| format!("exclude_{i}_{s}");
        let Some(t) = kf.get_string("Database", &k("type")) else {
            break;
        };
        if t.trim().is_empty() {
            break;
        }
        let pattern = kf.get_string("Database", &k("pattern")).unwrap_or_default();
        m.add(Exclude::new(
            &pattern,
            kf.get_bool("Database", &k("active")).unwrap_or(false),
            ExcludeType::parse_str(&t),
            Scope::parse_str(
                &kf.get_string("Database", &k("match_scope"))
                    .unwrap_or_default(),
            ),
            Target::parse_str(&kf.get_string("Database", &k("target")).unwrap_or_default()),
        ));
    }
    m.exclude_hidden = kf
        .get_bool("Database", "exclude_hidden_files_and_folders")
        .unwrap_or(false);
    m
}

fn legacy_includes(kf: &KeyFile) -> Vec<Include> {
    let mut v = Vec::new();
    for pos in 1.. {
        let Some(path) = kf.get_string("Database", &format!("location_{pos}")) else {
            break;
        };
        let enabled = kf
            .get_bool("Database", &format!("location_enabled_{pos}"))
            .unwrap_or(false);
        let one_fs = kf
            .get_bool("Database", &format!("location_one_filesystem_{pos}"))
            .unwrap_or(false);
        add_include(
            &mut v,
            Include::new(&path, enabled, one_fs, false, false, 0),
        );
    }
    v
}

fn legacy_excludes(kf: &KeyFile) -> ExcludeManager {
    let mut m = ExcludeManager::default();
    for pos in 1.. {
        let Some(path) = kf.get_string("Database", &format!("exclude_location_{pos}")) else {
            break;
        };
        let enabled = kf
            .get_bool("Database", &format!("exclude_location_enabled_{pos}"))
            .unwrap_or(false);
        m.add(Exclude::new(
            &path,
            enabled,
            ExcludeType::Fixed,
            Scope::FullPath,
            Target::Folders,
        ));
    }
    if let Some(files) = kf.get_string("Database", "exclude_files") {
        for f in files.split(';') {
            m.add(Exclude::new(
                f,
                true,
                ExcludeType::Fixed,
                Scope::Basename,
                Target::Both,
            ));
        }
    }
    m.exclude_hidden = kf
        .get_bool("Database", "exclude_hidden_files_and_folders")
        .unwrap_or(false);
    m
}
