//! Folder monitoring (wave 2): watch the includes marked "monitor" and rescan after changes.
//!
//! The original applies inotify/fanotify events to the index entry by entry
//! (`fsearch_folder_monitor_*.c`, `fsearch_database_rescan_manager.c`). This port watches the same
//! folders with `notify` (inotify) and runs a debounced rescan, which the parallel scanner makes
//! cheap; see ADR-013.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;

use fsearch_core::rules::Include;
use notify::{RecursiveMode, Watcher};

use super::backend::Request;

pub struct Monitor {
    _watcher: Option<notify::RecommendedWatcher>,
    stop: Arc<AtomicBool>,
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Paths whose changes must not trigger a rescan (our own database and config).
fn ignored(p: &Path, ignore: &[PathBuf]) -> bool {
    ignore.iter().any(|i| p.starts_with(i))
}

pub fn start(
    includes: &[Include],
    tx: Sender<Request>,
    repaint: Arc<dyn Fn() + Send + Sync>,
) -> Monitor {
    let stop = Arc::new(AtomicBool::new(false));
    let dirty = Arc::new(AtomicBool::new(false));
    let roots: Vec<&Include> = includes
        .iter()
        .filter(|i| i.active && i.monitored)
        .collect();
    if roots.is_empty() {
        return Monitor {
            _watcher: None,
            stop,
        };
    }
    let mut ignore = Vec::new();
    if let Some(d) = fsearch_core::config::database_path().parent() {
        ignore.push(d.to_path_buf());
    }
    ignore.push(fsearch_core::config::config_dir());
    let d2 = dirty.clone();
    let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res
            && ev.paths.iter().any(|p| !ignored(p, &ignore))
        {
            d2.store(true, Ordering::Relaxed);
        }
    });
    let Ok(mut watcher) = watcher else {
        return Monitor {
            _watcher: None,
            stop,
        };
    };
    for inc in roots {
        let _ = watcher.watch(Path::new(&inc.path), RecursiveMode::Recursive);
    }
    let s2 = stop.clone();
    std::thread::spawn(move || {
        while !s2.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(1500));
            if dirty.swap(false, Ordering::Relaxed) {
                if tx.send(Request::RescanAuto).is_err() {
                    break;
                }
                repaint();
            }
        }
    });
    Monitor {
        _watcher: Some(watcher),
        stop,
    }
}

/// Periodic rescans for includes with `rescan_after` (seconds) set.
pub fn start_periodic(includes: &[Include], tx: Sender<Request>) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let period = includes
        .iter()
        .filter(|i| i.active && i.rescan_after > 0)
        .map(|i| i.rescan_after)
        .min();
    if let Some(secs) = period {
        let s2 = stop.clone();
        std::thread::spawn(move || {
            let mut waited = 0i64;
            while !s2.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_secs(1));
                waited += 1;
                if waited >= secs {
                    waited = 0;
                    if tx.send(Request::RescanAuto).is_err() {
                        break;
                    }
                }
            }
        });
    }
    stop
}
