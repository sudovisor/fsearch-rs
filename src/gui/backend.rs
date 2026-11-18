//! The database worker: loads or scans the index, answers searches and sorts off the UI thread
//! (port of `FsearchDatabase` and its work queue, `fsearch_database.c` / `fsearch_database_work.c`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fsearch_core::config::{Config, database_path};
use fsearch_core::db::{FLAG_DEFAULT, Prop};
use fsearch_core::filter::Filter;
use fsearch_core::{Query, SortDir, Store, View, file};

pub struct SearchRequest {
    pub id: u64,
    pub text: String,
    pub filter: Option<Filter>,
    pub filters: Vec<Filter>,
    pub flags: u32,
    pub sort: Prop,
    pub dir: SortDir,
}

pub enum Request {
    Search(SearchRequest),
    Sort {
        id: u64,
        prop: Prop,
        dir: SortDir,
    },
    Rescan,
    /// rescan asked for by folder monitoring or the periodic timer (throttled)
    RescanAuto,
    SetConfig(Box<Config>),
    SaveAndQuit,
}

pub struct Results {
    pub id: u64,
    pub store: Arc<Store>,
    pub view: Arc<View>,
    pub query: Arc<Query>,
    pub elapsed: Duration,
}

pub enum Event {
    Loading,
    Scanning,
    Ready { folders: usize, files: usize },
    Results(Results),
    Sorting,
    Querying,
    Saved,
}

pub struct Backend {
    pub tx: Sender<Request>,
    pub rx: Receiver<Event>,
    pub cancel: Arc<Mutex<Arc<AtomicBool>>>,
    pub scan_progress: Arc<AtomicU64>,
}

struct Worker {
    events: Sender<Event>,
    repaint: Box<dyn Fn() + Send>,
    store: Option<Arc<Store>>,
    config: Config,
    db_path: PathBuf,
    last_search: Option<SearchRequest>,
    last: Option<(Arc<Query>, View)>,
    cancel: Arc<Mutex<Arc<AtomicBool>>>,
    progress: Arc<AtomicU64>,
    scan_cancel: Arc<AtomicBool>,
    last_scan: Option<(Instant, Duration)>,
    auto_pending: bool,
    self_tx: Sender<Request>,
}

impl Worker {
    fn send(&self, e: Event) {
        let _ = self.events.send(e);
        (self.repaint)();
    }

    fn current_cancel(&self) -> Arc<AtomicBool> {
        self.cancel
            .lock()
            .map(|c| c.clone())
            .unwrap_or_else(|_| Arc::new(AtomicBool::new(false)))
    }

    fn load_or_scan(&mut self, force_scan: bool) {
        if !force_scan {
            self.send(Event::Loading);
            if let Ok(store) = file::load(
                &self.db_path,
                Some((&self.config.includes, &self.config.excludes)),
            ) {
                self.set_store(store);
                if self
                    .config
                    .includes
                    .iter()
                    .any(|i| i.active && i.scan_after_launch)
                {
                    self.rescan();
                }
                return;
            }
        }
        self.rescan();
    }

    fn rescan(&mut self) {
        // Folders that change all the time (caches, logs) must not keep the CPU busy: leave at
        // least four times the last scan's duration (and one second) between scans.
        let started = Instant::now();
        self.send(Event::Scanning);
        self.progress.store(0, Ordering::Relaxed);
        self.scan_cancel.store(false, Ordering::Relaxed);
        let store = Store::scan(
            self.config.includes.clone(),
            self.config.excludes.clone(),
            FLAG_DEFAULT,
            &self.scan_cancel,
            &self.progress,
        );
        if let Some(store) = store {
            if let Some(dir) = self.db_path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = file::save(&store, &self.db_path);
            self.set_store(store);
            self.last_scan = Some((Instant::now(), started.elapsed()));
        } else if let Some(s) = &self.store {
            self.send(Event::Ready {
                folders: s.num_folders(),
                files: s.num_files(),
            });
        }
    }

    fn set_store(&mut self, store: Store) {
        let s = Arc::new(store);
        self.send(Event::Ready {
            folders: s.num_folders(),
            files: s.num_files(),
        });
        self.store = Some(s);
        self.last = None; // results of the old index cannot be refined
        if let Some(req) = self.last_search.take() {
            self.search(req);
        }
    }

    fn search(&mut self, req: SearchRequest) {
        let Some(store) = self.store.clone() else {
            self.last_search = Some(req);
            return;
        };
        self.send(Event::Querying);
        let cancel = self.current_cancel();
        let t = Instant::now();
        let query = Arc::new(Query::new(
            &req.text,
            req.filter.as_ref(),
            &req.filters,
            req.flags,
        ));
        // typing narrows the query: filter the previous results instead of the whole index
        let view = match &self.last {
            Some((pq, pv)) => store.search_refine(pq, pv, &query, req.sort, req.dir, Some(&cancel)),
            None => store.search(&query, req.sort, req.dir, Some(&cancel)),
        };
        if cancel.load(Ordering::Relaxed) {
            self.last_search = Some(req);
            return;
        }
        let elapsed = t.elapsed();
        self.last = Some((query.clone(), view.clone()));
        self.send(Event::Results(Results {
            id: req.id,
            store,
            view: Arc::new(view),
            query,
            elapsed,
        }));
        self.last_search = Some(req);
    }

    fn sort(&mut self, id: u64, prop: Prop, dir: SortDir) {
        let (Some(store), Some((query, mut view))) = (self.store.clone(), self.last.take()) else {
            return;
        };
        self.send(Event::Sorting);
        let t = Instant::now();
        store.sort_view(&mut view, prop, dir, None);
        if let Some(r) = self.last_search.as_mut() {
            r.sort = prop;
            r.dir = dir;
        }
        self.last = Some((query.clone(), view.clone()));
        self.send(Event::Results(Results {
            id,
            store,
            view: Arc::new(view),
            query,
            elapsed: t.elapsed(),
        }));
    }
}

/// Start the worker thread. `repaint` wakes the UI when an event is ready.
pub fn start(config: Config, update_now: bool, repaint: Box<dyn Fn() + Send>) -> Backend {
    let (tx, rx_req) = channel::<Request>();
    let (tx_ev, rx) = channel::<Event>();
    let cancel = Arc::new(Mutex::new(Arc::new(AtomicBool::new(false))));
    let progress = Arc::new(AtomicU64::new(0));
    let mut w = Worker {
        events: tx_ev,
        repaint,
        store: None,
        config,
        db_path: database_path(),
        last_search: None,
        last: None,
        cancel: cancel.clone(),
        progress: progress.clone(),
        scan_cancel: Arc::new(AtomicBool::new(false)),
        last_scan: None,
        auto_pending: false,
        self_tx: tx.clone(),
    };
    std::thread::Builder::new()
        .name("fsearch-db".into())
        .spawn(move || {
            w.load_or_scan(update_now);
            while let Ok(mut req) = rx_req.recv() {
                // only the newest search matters; repeated rescans collapse into one
                while let Ok(next) = rx_req.try_recv() {
                    match (&req, &next) {
                        (Request::Search(_), Request::Search(_))
                        | (Request::Rescan, Request::Rescan)
                        | (Request::RescanAuto, Request::RescanAuto) => req = next,
                        _ => {
                            w.handle(req);
                            req = next;
                        }
                    }
                }
                if !w.handle(req) {
                    break;
                }
            }
        })
        .ok();
    Backend {
        tx,
        rx,
        cancel,
        scan_progress: progress,
    }
}

impl Worker {
    fn handle(&mut self, req: Request) -> bool {
        match req {
            Request::Search(s) => self.search(s),
            Request::Sort { id, prop, dir } => self.sort(id, prop, dir),
            Request::Rescan => self.rescan(),
            Request::RescanAuto => {
                // Folders that change all the time (caches, logs) must not keep the CPU busy:
                // leave at least four times the last scan's duration (and 2 s) between them.
                let wait = self.last_scan.and_then(|(end, took)| {
                    let gap = (took * 4).max(Duration::from_secs(2));
                    gap.checked_sub(end.elapsed())
                });
                match wait {
                    Some(d) if !self.auto_pending => {
                        self.auto_pending = true;
                        let tx = self.self_tx.clone();
                        std::thread::spawn(move || {
                            std::thread::sleep(d);
                            let _ = tx.send(Request::RescanAuto);
                        });
                    }
                    Some(_) => {}
                    None => {
                        self.auto_pending = false;
                        self.rescan();
                    }
                }
            }
            Request::SetConfig(c) => {
                let db_changed =
                    c.includes != self.config.includes || c.excludes != self.config.excludes;
                self.config = *c;
                if db_changed {
                    self.rescan();
                }
            }
            Request::SaveAndQuit => {
                self.send(Event::Saved);
                return false;
            }
        }
        true
    }
}

impl Backend {
    /// Cancel the running search; the next one gets a fresh flag.
    pub fn new_search_token(&self) {
        if let Ok(mut c) = self.cancel.lock() {
            c.store(true, Ordering::Relaxed);
            *c = Arc::new(AtomicBool::new(false));
        }
    }
}
