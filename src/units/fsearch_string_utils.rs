//! Glue from the front door to the Rust core: the headless driver commands the recorded cases use
//! (`dump`, `query`, `resave`, `scandump`, `scanmd5`), answered by `fsearch-core` (which is built on
//! the `fsearch-string-utils-rs` unit). Output matches `rust/harness/fsdriver` (the C original).
//!
//! The arguments are taken from argv directly: option values (`--exclude <spec>`, `--resort <prop>`)
//! must stay in order with the positional arguments.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicU64};

use clap::ArgMatches;
use fsearch_core::db::{FLAG_DEFAULT, Id, Prop};
use fsearch_core::filter::default_filters;
use fsearch_core::rules::{
    Exclude, ExcludeManager, ExcludeType, Include, Scope, Target, add_include,
};
use fsearch_core::{Query, SortDir, Store, View, file};
use md5::{Digest, Md5};

pub fn run(command: &str, _matches: &ArgMatches) -> ExitCode {
    let argv: Vec<Vec<u8>> = std::env::args_os().skip(2).map(os_bytes).collect();
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    let code = match command {
        "dump" => dump(&argv, &mut out),
        "query" => query(&argv, &mut out),
        "resave" => resave(&argv, &mut out),
        "scandump" => scandump(&argv, &mut out),
        "scanmd5" => scanmd5(&argv, &mut out),
        _ => {
            drop(out);
            return ExitCode::from(app_main());
        }
    };
    let _ = out.flush();
    ExitCode::from(code)
}

/// Command-line arguments as bytes (raw on Unix, UTF-8 elsewhere).
fn os_bytes(s: OsString) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        s.into_vec()
    }
    #[cfg(not(unix))]
    {
        s.to_string_lossy().into_owned().into_bytes()
    }
}

type Out<'a> = std::io::BufWriter<std::io::StdoutLock<'a>>;

fn path_of(b: &[u8]) -> PathBuf {
    crate::gui::path_from_bytes(b)
}

fn parse_sort(s: &[u8]) -> Option<Prop> {
    Some(match s {
        b"name" => Prop::Name,
        b"path" => Prop::Path,
        b"size" => Prop::Size,
        b"mtime" => Prop::Mtime,
        b"ext" => Prop::Extension,
        b"type" => Prop::FileType,
        _ => return None,
    })
}

fn print_entry(store: &Store, id: Id, out: &mut Out) {
    let e = store.db.entry(id);
    let _ = write!(
        out,
        "{} {} {} ",
        if id.is_folder() { 'D' } else { 'F' },
        e.size,
        e.mtime
    );
    let _ = out.write_all(&store.db.full_path(id));
    let _ = out.write_all(b"\n");
}

fn print_view(store: &Store, view: &View, out: &mut Out) {
    let _ = writeln!(
        out,
        "= {} results ({} folders, {} files)",
        view.len(),
        view.folders.len(),
        view.files.len()
    );
    for i in 0..view.len() {
        if let Some(id) = view.get(i) {
            print_entry(store, id, out);
        }
    }
}

/// Like the driver: load a private copy, because the database is locked exclusively even for
/// reading and cases run in parallel.
fn load(db: &[u8], out: &mut Out) -> Option<Store> {
    let fail = |out: &mut Out| {
        let _ = writeln!(out, "load failed");
        None
    };
    let Ok(data) = std::fs::read(path_of(db)) else {
        return fail(out);
    };
    let tmp = std::env::temp_dir().join(format!("fsearch-rs-{}.db", std::process::id()));
    if std::fs::write(&tmp, data).is_err() {
        return fail(out);
    }
    let res = file::load(&tmp, None);
    let _ = std::fs::remove_file(&tmp);
    match res {
        Ok(s) => Some(s),
        Err(_) => fail(out),
    }
}

fn dump(argv: &[Vec<u8>], out: &mut Out) -> u8 {
    let Some(db) = argv.first() else { return 2 };
    let Some(store) = load(db, out) else { return 1 };
    let q = Query::new("", None, &[], 0);
    let view = store.search(&q, Prop::Path, SortDir::Ascending, None);
    print_view(&store, &view, out);
    0
}

fn query(argv: &[Vec<u8>], out: &mut Out) -> u8 {
    if argv.len() < 5 {
        return 2;
    }
    let (Some(sort), dir) = (
        parse_sort(&argv[1]),
        if argv[2] == b"desc" {
            SortDir::Descending
        } else {
            SortDir::Ascending
        },
    ) else {
        eprintln!("bad sort: {}", String::from_utf8_lossy(&argv[1]));
        return 2;
    };
    let flags = std::str::from_utf8(&argv[3])
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0) as u32;
    let filters = default_filters();
    let filter = if argv[4] == b"-" {
        None
    } else {
        filters
            .iter()
            .find(|f| f.name.as_bytes() == argv[4].as_slice())
    };
    let mut i = 5;
    let mut resort = None;
    if i + 1 < argv.len() && argv[i] == b"--resort" {
        let Some(p) = parse_sort(&argv[i + 1]) else {
            eprintln!("bad sort");
            return 2;
        };
        resort = Some(p);
        i += 2;
    }
    let Some(store) = load(&argv[0], out) else {
        return 1;
    };
    for q in &argv[i..] {
        let query = Query::new_bytes(q, filter, &filters, flags);
        let _ = out.write_all(b"# query \"");
        let _ = out.write_all(q);
        let _ = out.write_all(b"\"\n");
        let mut view = store.search(&query, sort, dir, None);
        if let Some(p) = resort {
            store.sort_view(&mut view, p, dir, None);
        }
        print_view(&store, &view, out);
    }
    0
}

fn scan_store(argv: &[Vec<u8>]) -> Store {
    let mut includes: Vec<Include> = Vec::new();
    let mut excludes = ExcludeManager::default();
    let mut i = 0;
    while i < argv.len() {
        let a = &argv[i];
        if a == b"--exclude-hidden" {
            excludes.exclude_hidden = true;
        } else if a == b"--exclude" && i + 1 < argv.len() {
            i += 1;
            let spec = String::from_utf8_lossy(&argv[i]).into_owned();
            let parts: Vec<&str> = spec.splitn(4, ':').collect();
            let get = |k: usize| parts.get(k).copied().unwrap_or("");
            excludes.add(Exclude::new(
                get(3),
                true,
                ExcludeType::parse_str(get(0)),
                Scope::parse_str(get(1)),
                Target::parse_str(get(2)),
            ));
        } else {
            add_include(
                &mut includes,
                Include::new(&String::from_utf8_lossy(a), true, false, false, false, 0),
            );
        }
        i += 1;
    }
    let cancel = AtomicBool::new(false);
    let progress = AtomicU64::new(0);
    let mut store = Store::scan(includes.clone(), excludes, FLAG_DEFAULT, &cancel, &progress)
        .unwrap_or_else(|| Store::empty(includes, ExcludeManager::default(), FLAG_DEFAULT));
    for inc in &mut store.includes {
        inc.last_scan_time = 0;
        inc.last_scan_duration = 0;
    }
    store
}

fn print_md5(path: &Path, out: &mut Out) {
    match std::fs::read(path) {
        Ok(data) => {
            let sum = Md5::digest(&data);
            let hex: String = sum.iter().map(|b| format!("{b:02x}")).collect();
            let _ = writeln!(out, "bytes {} md5 {}", data.len(), hex);
        }
        Err(_) => {
            let _ = writeln!(out, "md5 <unreadable>");
        }
    }
}

fn scandump(argv: &[Vec<u8>], out: &mut Out) -> u8 {
    let store = scan_store(argv);
    let q = Query::new("", None, &[], 0);
    let view = store.search(&q, Prop::Path, SortDir::Ascending, None);
    print_view(&store, &view, out);
    0
}

fn scanmd5(argv: &[Vec<u8>], out: &mut Out) -> u8 {
    let Some(db) = argv.first() else { return 2 };
    let store = scan_store(&argv[1..]);
    let path = path_of(db);
    if file::save(&store, &path).is_err() {
        let _ = writeln!(out, "save failed");
        return 1;
    }
    print_md5(&path, out);
    0
}

fn resave(argv: &[Vec<u8>], out: &mut Out) -> u8 {
    if argv.len() < 2 {
        return 2;
    }
    let Some(store) = load(&argv[0], out) else {
        return 1;
    };
    let path = path_of(&argv[1]);
    if file::save(&store, &path).is_err() {
        let _ = writeln!(out, "save failed");
        return 1;
    }
    print_md5(&path, out);
    0
}

const USAGE: &str = "Usage:
  fsearch [OPTION…]

Help Options:
  -h, --help                 Show help options
  --help-all                 Show all help options
  --help-gapplication        Show GApplication options
  --help-gtk                 Show GTK+ Options

Application Options:
  --new-window               Open a new application window
  --minimized                Minimize the application window
  --preferences              Show the application preferences
  -s, --search=PATTERN       Set the search pattern
  -u, --update-database      Update the database and exit
  -v, --version              Print version information and exit
  --display=DISPLAY          X display to use

";

/// The application entry point (`fsearch.c`): GApplication-style options, a single running
/// instance, then the window.
fn app_main() -> u8 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut opts = crate::gui::StartOptions {
        search: None,
        minimized: false,
        preferences: false,
        update: false,
    };
    let mut version = false;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-h" | "--help" | "--help-all" => {
                print!("{USAGE}");
                return 0;
            }
            "-v" | "--version" => version = true,
            "-u" | "--update-database" => opts.update = true,
            "--new-window" => {}
            "--minimized" => opts.minimized = true,
            "--preferences" => opts.preferences = true,
            "-s" | "--search" => {
                i += 1;
                match args.get(i) {
                    Some(v) => opts.search = Some(v.clone()),
                    None => {
                        eprintln!("Missing argument for {a}");
                        return 1;
                    }
                }
            }
            _ if a.starts_with("--search=") => {
                opts.search = Some(a["--search=".len()..].to_string())
            }
            _ if a.starts_with("-s") && a.len() > 2 => opts.search = Some(a[2..].to_string()),
            _ if a.starts_with("--display") => {}
            _ if a.starts_with('-') => {
                eprintln!("Unknown option {a}");
                return 1;
            }
            _ => {}
        }
        i += 1;
    }
    if opts.update {
        return update_database();
    }
    if version {
        println!("FSearch {}", crate::gui::VERSION);
        return 0;
    }
    let mut msgs = Vec::new();
    if let Some(s) = &opts.search {
        msgs.push(format!("search {s}"));
    }
    if opts.preferences {
        msgs.push("preferences".into());
    }
    if msgs.is_empty() {
        msgs.push("activate".into());
    }
    if crate::gui::ipc::send(&msgs) {
        return 0;
    }
    crate::gui::run(opts)
}

/// `--update-database`: ask a running instance, else scan here, save and exit.
fn update_database() -> u8 {
    if crate::gui::ipc::send(&["update".to_string()]) {
        return 0;
    }
    let c = fsearch_core::config::Config::load_or_default();
    let cancel = AtomicBool::new(false);
    let progress = AtomicU64::new(0);
    let Some(store) = Store::scan(c.includes, c.excludes, FLAG_DEFAULT, &cancel, &progress) else {
        return 1;
    };
    let path = fsearch_core::config::database_path();
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if file::save(&store, &path).is_err() {
        return 1;
    }
    0
}
