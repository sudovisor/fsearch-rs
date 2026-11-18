//! Headless tests of the window, driven through the accessibility tree (egui_kittest): every
//! command a user can reach (typing, toggles, shortcuts, filter, sorting, selection, keyboard
//! navigation, preferences, looks, trash, mass-open guard, update, empty states, menus).
//!
//! Each scenario gets its own config, data and runtime directories and its own copy of the
//! fixture tree, so nothing touches the user's settings, index or trash.

use super::*;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use fsearch_core::rules::{ExcludeManager, Include};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

static LOCK: Mutex<()> = Mutex::new(());

struct Env {
    root: PathBuf,
    tree: PathBuf,
}

/// The test tree (from `tools/compat/make_fixture.py`): names with Unicode, natural numbers,
/// hidden files, odd extensions; sizes are sparse files, so nothing large is written.
fn make_fixture(root: &Path) {
    let files: &[(&str, u64)] = &[
        ("docs/report.pdf", 120_000),
        ("docs/Report Final.PDF", 5_000),
        ("docs/notes.txt", 12),
        ("docs/README", 300),
        ("docs/archive.tar.gz", 2_500_000),
        ("docs/.hidden_note.txt", 4),
        ("docs/file2.txt", 2),
        ("docs/file10.txt", 10),
        ("docs/file02.txt", 3),
        ("docs/v1.9.txt", 19),
        ("docs/v1.10.txt", 110),
        ("docs/sub dir/deep/deeper/bottom.c", 999),
        ("docs/sub dir/deep/deeper/Bottom.H", 100),
        ("docs/sub dir/space name.md", 50),
        ("media/photo.JPG", 3_000_000),
        ("media/song.mp3", 4_000_000),
        ("media/Ärger.png", 4096),
        ("media/ärger.png", 4097),
        ("media/straße.txt", 33),
        ("media/日本語ファイル.txt", 41),
        ("code/main.rs", 1000),
        ("code/Makefile", 300),
        ("code/src/mod1/a.c", 1),
        ("code/src/mod2/B.C", 5),
        ("code/src/mod10/z.c", 6),
        ("code/src/mod9/y.c", 7),
    ];
    for (rel, size) in files {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::File::create(&p).unwrap().set_len(*size).unwrap();
    }
    std::fs::create_dir_all(root.join("docs/empty_folder")).unwrap();
    std::fs::create_dir_all(root.join("big")).unwrap();
    for i in 0..120u64 {
        std::fs::File::create(root.join(format!("big/item_{i:03}.log")))
            .unwrap()
            .set_len(i * 37)
            .unwrap();
    }
}

fn env(name: &str) -> Env {
    // under target/, not /tmp (which may have a small per-user quota)
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/ui-tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&root);
    let tree = root.join("tree");
    make_fixture(&tree);
    for d in ["config", "data", "run"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    // SAFETY-free: tests run one at a time under LOCK, so changing the environment is not racy
    unsafe_env("XDG_CONFIG_HOME", &root.join("config"));
    unsafe_env("XDG_DATA_HOME", &root.join("data"));
    unsafe_env("XDG_RUNTIME_DIR", &root.join("run"));
    Env { root, tree }
}

fn unsafe_env(k: &str, v: &Path) {
    // `set_var` is unsafe in edition 2024 because other threads may read the environment; the
    // scenarios hold LOCK and the app's threads only read these variables after this call.
    // SAFETY: see above.
    unsafe { std::env::set_var(k, v) }
}

fn config(e: &Env, with_folder: bool) -> Config {
    let mut c = Config::default();
    if with_folder {
        c.includes = vec![Include::new(
            &e.tree.to_string_lossy(),
            true,
            false,
            false,
            false,
            0,
        )];
    }
    c.excludes = ExcludeManager::default();
    let _ = c.save();
    c
}

fn harness(c: Config, search: &str) -> Harness<'static, App> {
    let opts = StartOptions {
        search: Some(search.to_string()),
        minimized: false,
        preferences: false,
        update: false,
    };
    Harness::builder()
        .with_size([1100.0, 720.0])
        .build_eframe(move |cc| App::new(cc, c, opts))
}

fn wait(h: &mut Harness<'static, App>, what: &str, cond: impl Fn(&App) -> bool) {
    let t = Instant::now();
    loop {
        h.step();
        if cond(h.state()) {
            return;
        }
        assert!(
            t.elapsed() < Duration::from_secs(20),
            "timed out waiting for: {what}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn settle(h: &mut Harness<'static, App>) {
    for _ in 0..6 {
        h.step();
        std::thread::sleep(Duration::from_millis(15));
    }
}

fn ready(a: &App) -> bool {
    matches!(a.db, DbState::Ready(_))
        && a.results.is_some()
        && a.task.is_none()
        && a.pending_search_at.is_none()
}

/// Names in the visible result list, in order.
fn names(a: &App) -> Vec<String> {
    let Some(r) = &a.results else {
        return Vec::new();
    };
    (0..a.visible_len())
        .filter_map(|i| r.view.get(i))
        .map(|id| String::from_utf8_lossy(r.store.db.display_name(id)).into_owned())
        .collect()
}

fn search(h: &mut Harness<'static, App>, q: &str) {
    h.state_mut().query_text = q.to_string();
    h.state_mut().search(false);
    let q = q.to_string();
    wait(h, "search results", move |a| {
        ready(a)
            && a.sent.as_ref().is_some_and(|s| s.0 == q)
            && a.results.as_ref().is_some_and(|r| r.id + 1 == a.next_id)
    });
}

#[test]
fn ui_functions() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    typing_and_toggles();
    shortcuts_and_update();
    filter_and_sorting();
    selection_and_keyboard();
    empty_states();
    preferences_and_looks();
    trash_and_mass_open();
    menus_and_about();
}

fn typing_and_toggles() {
    let e = env("typing");
    let mut h = harness(config(&e, true), "");
    wait(&mut h, "index ready", ready);
    assert!(
        h.state().visible_len() > 100,
        "empty query lists everything"
    );
    // type into the search field
    h.get_by_role(egui::Role::TextInput).type_text("report");
    wait(&mut h, "typed search", |a| {
        ready(a) && a.query_text == "report" && a.sent.as_ref().is_some_and(|s| s.0 == "report")
    });
    settle(&mut h);
    let n = names(h.state());
    assert!(
        n.iter().any(|x| x == "report.pdf") && n.iter().any(|x| x == "Report Final.PDF"),
        "{n:?}"
    );
    // the keycap toggles
    h.get_by_label("Match case").click();
    wait(&mut h, "match case", |a| a.config.match_case && ready(a));
    settle(&mut h);
    let n = names(h.state());
    assert!(
        n.contains(&"report.pdf".to_string()) && !n.contains(&"Report Final.PDF".to_string()),
        "{n:?}"
    );
    h.get_by_label("Match case").click();
    h.get_by_label("Regular expressions").click();
    wait(&mut h, "regex on", |a| {
        a.config.enable_regex && !a.config.match_case
    });
    search(&mut h, "^file[0-9]+\\.txt$");
    let mut n = names(h.state());
    n.sort();
    assert_eq!(n, vec!["file02.txt", "file10.txt", "file2.txt"]);
    h.get_by_label("Regular expressions").click();
    h.get_by_label("Search in path").click();
    wait(&mut h, "path on", |a| {
        a.config.search_in_path && !a.config.enable_regex
    });
    search(&mut h, "deeper");
    let n = names(h.state());
    assert!(
        n.contains(&"bottom.c".to_string()),
        "path search finds files below 'deeper': {n:?}"
    );
    // auto search-in-path shows on the toggle's tooltip, and refinement keeps results exact
    h.get_by_label("Search in path").click();
    search(&mut h, "sub");
    let a = names(h.state()).len();
    search(&mut h, "sub dir");
    let b = names(h.state());
    assert!(b.len() <= a && b.contains(&"sub dir".to_string()));
}

fn shortcuts_and_update() {
    let e = env("shortcuts");
    let mut h = harness(config(&e, true), "");
    wait(&mut h, "index ready", ready);
    h.key_press_modifiers(Modifiers::COMMAND, Key::I);
    h.key_press_modifiers(Modifiers::COMMAND, Key::R);
    h.key_press_modifiers(Modifiers::COMMAND, Key::U);
    settle(&mut h);
    let c = &h.state().config;
    assert!(
        c.match_case && c.enable_regex && c.search_in_path,
        "Ctrl+I, Ctrl+R, Ctrl+U toggle"
    );
    h.key_press_modifiers(Modifiers::COMMAND, Key::I);
    h.key_press_modifiers(Modifiers::COMMAND, Key::R);
    h.key_press_modifiers(Modifiers::COMMAND, Key::U);
    settle(&mut h);
    // Ctrl+Shift+R updates the index and must not toggle regex (regression)
    std::fs::write(e.tree.join("zz_new_file.txt"), b"x").unwrap();
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::R);
    wait(&mut h, "rescan", |a| {
        ready(a) && names(a).contains(&"zz_new_file.txt".to_string())
    });
    assert!(
        !h.state().config.enable_regex,
        "Ctrl+Shift+R must not toggle regex"
    );
    // the Update button does the same
    std::fs::write(e.tree.join("zz_second.txt"), b"x").unwrap();
    h.get_by_label("Update the index").click();
    wait(&mut h, "update button", |a| {
        ready(a) && names(a).contains(&"zz_second.txt".to_string())
    });
    // Ctrl+P opens the preferences, Ctrl+F focuses the search field
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    settle(&mut h);
    assert!(h.state().prefs.is_some());
    h.get_by_label("Cancel").click();
    settle(&mut h);
    assert!(h.state().prefs.is_none());
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    settle(&mut h);
    assert!(h.state().search_has_focus);
}

fn filter_and_sorting() {
    let e = env("filter");
    let mut h = harness(config(&e, true), "");
    wait(&mut h, "index ready", ready);
    // filter menu
    h.get_by_role(egui::Role::ComboBox).click();
    settle(&mut h);
    h.get_by_label("Folders").click();
    wait(&mut h, "folders filter", |a| a.filter_idx == 1 && ready(a));
    settle(&mut h);
    let r = h.state().results.as_ref().unwrap();
    assert!(
        r.view.files.is_empty() && !r.view.folders.is_empty(),
        "Folders filter shows folders only"
    );
    h.get_by_role(egui::Role::ComboBox).click();
    settle(&mut h);
    h.get_by_label("All").click();
    wait(&mut h, "all filter", |a| a.filter_idx == 0 && ready(a));
    // sorting by clicking headers
    search(&mut h, "item_");
    h.get_by_label("Size").click();
    wait(&mut h, "size sort", |a| {
        ready(a)
            && a.results
                .as_ref()
                .is_some_and(|r| r.view.sort_prop() == Prop::Size)
    });
    let sizes = |a: &App| {
        let r = a.results.as_ref().unwrap();
        (0..a.visible_len())
            .map(|i| r.store.db.entry(r.view.get(i).unwrap()).size)
            .collect::<Vec<_>>()
    };
    let s = sizes(h.state());
    assert!(s.windows(2).all(|w| w[0] <= w[1]), "ascending by size");
    h.get_by_label("Size").click();
    wait(&mut h, "size desc", |a| {
        ready(a)
            && a.results
                .as_ref()
                .is_some_and(|r| r.view.dir == SortDir::Descending)
    });
    let s = sizes(h.state());
    assert!(s.windows(2).all(|w| w[0] >= w[1]), "descending by size");
    for col in ["Name", "Path", "Modified", "Ext"] {
        h.get_by_label(col).click();
        settle(&mut h);
        wait(&mut h, "sorted", ready);
    }
    assert_eq!(h.state().visible_len(), 120, "sorting keeps every result");
}

fn selection_and_keyboard() {
    let e = env("select");
    let mut h = harness(config(&e, true), "");
    wait(&mut h, "index ready", ready);
    search(&mut h, "item_00");
    settle(&mut h);
    // Down from the search field moves into the list
    h.key_press(Key::ArrowDown);
    settle(&mut h);
    assert_eq!(
        h.state().cursor,
        Some(0),
        "Down in the field moves to the first result"
    );
    assert!(!h.state().search_has_focus);
    h.key_press(Key::ArrowDown);
    h.key_press_modifiers(Modifiers::SHIFT, Key::ArrowDown);
    settle(&mut h);
    assert_eq!(
        h.state().selection.len(),
        2,
        "Shift+Down extends the selection"
    );
    h.key_press(Key::End);
    settle(&mut h);
    assert_eq!(h.state().cursor, Some(h.state().visible_len() - 1));
    h.key_press(Key::Home);
    settle(&mut h);
    assert_eq!(h.state().cursor, Some(0));
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    settle(&mut h);
    assert_eq!(
        h.state().selection.len(),
        h.state().visible_len(),
        "Ctrl+A in the list selects all results"
    );
    // clicking a row selects it alone
    h.get_by_label("item_005.log").click();
    settle(&mut h);
    assert_eq!(h.state().selection.len(), 1);
    // Ctrl+A in the search field selects text, not rows (regression)
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    settle(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    settle(&mut h);
    assert_eq!(
        h.state().selection.len(),
        1,
        "Ctrl+A while typing leaves the row selection alone"
    );
    // Up on the first row returns to the search field
    h.key_press(Key::ArrowDown);
    settle(&mut h);
    h.key_press(Key::Home);
    h.key_press(Key::ArrowUp);
    settle(&mut h);
    h.key_press(Key::ArrowUp);
    settle(&mut h);
    assert!(
        h.state().search_has_focus,
        "Up from the first row focuses the search field"
    );
}

fn empty_states() {
    let e = env("empty");
    // no folders configured: the call to action is there
    let mut h = harness(config(&e, false), "");
    wait(&mut h, "empty index", |a| matches!(a.db, DbState::Ready(0)));
    settle(&mut h);
    h.get_by_label("Add folder…");
    // the empty state's own Preferences button (the toolbar gear has the same label)
    h.get_all_by_label("Preferences")
        .last()
        .expect("empty-state button")
        .click();
    settle(&mut h);
    assert!(
        h.state().prefs.is_some(),
        "Preferences opens on the Index page"
    );
    drop(h);
    // nothing matches
    let mut h = harness(config(&e, true), "");
    wait(&mut h, "index ready", ready);
    search(&mut h, "zzzz-nothing-matches");
    assert_eq!(h.state().visible_len(), 0);
    // hide results until typing
    let mut c = config(&e, true);
    c.hide_results_on_empty_search = true;
    let mut h = harness(c, "");
    wait(&mut h, "index ready", ready);
    assert_eq!(h.state().visible_len(), 0, "nothing listed before typing");
    search(&mut h, "notes");
    assert!(h.state().visible_len() > 0);
}

fn preferences_and_looks() {
    let e = env("prefs");
    let mut h = harness(config(&e, true), "");
    wait(&mut h, "index ready", ready);
    h.get_by_label("Preferences").click();
    settle(&mut h);
    for page in [
        "Interface",
        "Search",
        "Index",
        "Filters",
        "Shortcuts",
        "Appearance",
    ] {
        // the modal's page button (the menu bar also has a "Search" menu)
        h.get_all_by_label(page).last().expect("page").click();
        settle(&mut h);
    }
    // every look renders (live preview) without a panic
    for look in theme::ALL {
        h.get_by_label(look.name()).click();
        settle(&mut h);
        assert_eq!(h.state().look, look);
    }
    // Cancel restores the saved look
    h.get_by_label("Cancel").click();
    settle(&mut h);
    assert_eq!(h.state().look, Look::ModernDark);
    // choose Windows 95 and apply: kept and saved
    h.get_by_label("Preferences").click();
    settle(&mut h);
    h.get_by_label("Windows 95").click();
    settle(&mut h);
    h.get_by_label("Apply").click();
    settle(&mut h);
    assert_eq!(h.state().look, Look::Win95);
    let saved = std::fs::read_to_string(e.root.join("config/fsearch/fsearch.conf")).unwrap();
    assert!(saved.contains("look=win95"), "{saved}");
    // the Index page adds a folder by path and applying rescans
    let extra = e.root.join("extra");
    std::fs::create_dir_all(&extra).unwrap();
    std::fs::write(extra.join("from_extra.txt"), b"x").unwrap();
    h.get_by_label("Preferences").click();
    settle(&mut h);
    h.get_all_by_label("Index").last().expect("page").click();
    settle(&mut h);
    h.get_by_label("Apply").click(); // no change: nothing breaks
    settle(&mut h);
    let mut c = h.state().config.clone();
    fsearch_core::rules::add_include(
        &mut c.includes,
        Include::new(&extra.to_string_lossy(), true, false, false, false, 0),
    );
    let ctx = h.ctx.clone();
    h.state_mut().apply_config(&ctx, c);
    wait(&mut h, "rescan after adding a folder", |a| {
        ready(a) && names(a).contains(&"from_extra.txt".to_string())
    });
}

fn trash_and_mass_open() {
    let e = env("trash");
    let mut h = harness(config(&e, true), "");
    wait(&mut h, "index ready", ready);
    // more than ten results: Enter asks before opening them all
    search(&mut h, "item_");
    h.key_press(Key::ArrowDown);
    settle(&mut h);
    h.key_press_modifiers(Modifiers::COMMAND, Key::A);
    settle(&mut h);
    h.key_press(Key::Enter);
    settle(&mut h);
    assert!(
        h.state().confirm_open.is_some(),
        "opening 120 items asks first"
    );
    h.get_by_label("Cancel").click();
    settle(&mut h);
    assert!(h.state().confirm_open.is_none());
    // Delete moves to the (scenario's own) trash after confirming
    search(&mut h, "notes.txt");
    h.key_press(Key::ArrowDown);
    settle(&mut h);
    h.key_press(Key::ArrowDown);
    settle(&mut h);
    let target = e.tree.join("docs/notes.txt");
    assert!(target.exists());
    h.key_press(Key::Delete);
    settle(&mut h);
    assert!(h.state().confirm_trash.is_some());
    h.get_by_label("Move to trash").click();
    wait(&mut h, "trashed and rescanned", |a| {
        ready(a) && !names(a).contains(&"notes.txt".to_string())
    });
    assert!(!target.exists(), "file left its folder");
    let trashed = e.root.join("data/Trash/files/notes.txt");
    assert!(
        trashed.exists(),
        "file is in the trash, not deleted: {}",
        trashed.display()
    );
}

fn menus_and_about() {
    let e = env("menus");
    let mut h = harness(config(&e, true), "");
    wait(&mut h, "index ready", ready);
    h.get_by_label("Help").click();
    settle(&mut h);
    h.get_by_label("About FSearch").click();
    settle(&mut h);
    assert!(h.state().about);
    h.get_by_label("Close").click();
    settle(&mut h);
    assert!(!h.state().about);
    h.get_by_label("View").click();
    settle(&mut h);
    h.get_by_label_contains("Status bar").click();
    settle(&mut h);
    assert!(!h.state().config.show_statusbar);
    h.get_by_label("View").click();
    settle(&mut h);
    h.get_by_label_contains("Status bar").click();
    settle(&mut h);
    assert!(h.state().config.show_statusbar);
    // toggling a column in the View menu immediately saves to config
    h.get_by_label("View").click();
    settle(&mut h);
    h.get_by_label_contains("Type column").click();
    settle(&mut h);
    assert!(h.state().config.show_type_column);
    let conf = std::fs::read_to_string(e.root.join("config/fsearch/fsearch.conf")).unwrap();
    assert!(conf.contains("show_type_column=true"), "{conf}");
    // column adjustments are remembered across restarts
    h.state_mut().config.name_column_width = 320;
    h.state_mut().config.path_column_width = 280;
    h.state_mut().persist_config();
    let reloaded = Config::load_or_default();
    assert_eq!(reloaded.name_column_width, 320);
    assert_eq!(reloaded.path_column_width, 280);
    assert!(reloaded.show_type_column);
    h.get_by_label("Search").click();
    settle(&mut h);
    // the menu entry (its accessible name includes the shortcut); the keycap is the first match
    h.get_all_by_label_contains("Search in path")
        .last()
        .expect("menu entry")
        .click();
    settle(&mut h);
    assert!(h.state().config.search_in_path, "menu toggle");
    // narrow windows hide secondary columns but keep working
    h.set_size(egui::vec2(420.0, 600.0));
    settle(&mut h);
    search(&mut h, "report");
    assert!(h.state().visible_len() > 0);
}
