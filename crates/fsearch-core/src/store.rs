//! Port of `FsearchDatabaseIndexStore`, `fsearch_database_sort.c` and
//! `FsearchDatabaseSearchView` (`src/fsearch_database_index_store.c`, `..._search_view.c`).

use crate::cmp::cmp_paths;
use crate::db::{Db, FAST_SORT, Id, NONE, NUM_PROPS, Prop, Ranks, folder_path_ranks};
use crate::query::{MatchData, Query};
use crate::rules::{ExcludeManager, Include};
use rayon::prelude::*;
use std::cmp::Ordering;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDir {
    Ascending,
    Descending,
}

#[derive(Clone, Default)]
pub struct Sorted {
    pub folders: Arc<Vec<u32>>,
    pub files: Arc<Vec<u32>>,
}

pub struct Store {
    pub db: Db,
    pub includes: Vec<Include>,
    pub excludes: ExcludeManager,
    pub flags: u64,
    pub sorted: Vec<Option<Sorted>>,
    ranks: OnceLock<Ranks>,
    types: OnceLock<TypeRanks>,
    folder_paths: OnceLock<crate::db::FolderPaths>,
}

/// File type descriptions, interned and ranked by `strcmp`.
pub struct TypeRanks {
    pub names: Vec<String>,
    pub file_type: Vec<u32>,
    pub rank: Vec<u32>,
}

const THRESHOLD_FOR_PARALLEL_SEARCH: usize = 1000;
const CHUNK: usize = 4096;

/// `fsearch_database_sort_order_chain_for_property`.
pub fn chain_for(p: Prop) -> Vec<Prop> {
    match p {
        Prop::Name => vec![Prop::Name, Prop::Path],
        Prop::Path => vec![Prop::Path, Prop::Name],
        Prop::PathFull => vec![Prop::PathFull],
        Prop::Size => vec![Prop::Size, Prop::Name, Prop::Path],
        Prop::Mtime => vec![Prop::Mtime, Prop::Name, Prop::Path],
        Prop::Extension => vec![Prop::Extension, Prop::Name, Prop::Path],
        Prop::FileType => vec![Prop::FileType],
        _ => vec![],
    }
}

fn chain_prepend(chain: &[Prop], p: Prop) -> Vec<Prop> {
    let mut v = vec![p];
    v.extend(chain.iter().copied().filter(|&c| c != p));
    v.truncate(NUM_PROPS);
    v
}

impl Store {
    pub fn empty(includes: Vec<Include>, excludes: ExcludeManager, flags: u64) -> Store {
        Store {
            db: Db::default(),
            includes,
            excludes,
            flags,
            sorted: vec![None; NUM_PROPS],
            ranks: OnceLock::new(),
            types: OnceLock::new(),
            folder_paths: OnceLock::new(),
        }
    }

    pub fn from_parts(
        db: Db,
        includes: Vec<Include>,
        excludes: ExcludeManager,
        flags: u64,
        sorted: Vec<Option<Sorted>>,
    ) -> Store {
        Store {
            db,
            includes,
            excludes,
            flags,
            sorted,
            ranks: OnceLock::new(),
            types: OnceLock::new(),
            folder_paths: OnceLock::new(),
        }
    }

    pub fn num_folders(&self) -> usize {
        self.db.folders.len()
    }

    pub fn num_files(&self) -> usize {
        self.db.files.len()
    }

    /// `fsearch_database_index_store_start`: scan every active include, then build the fast-sort
    /// arrays.
    pub fn scan(
        includes: Vec<Include>,
        excludes: ExcludeManager,
        flags: u64,
        cancel: &AtomicBool,
        progress: &AtomicU64,
    ) -> Option<Store> {
        let mut store = Store::empty(includes, excludes, flags);
        let mut seen: Vec<String> = Vec::new();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        for i in 0..store.includes.len() {
            if !store.includes[i].active {
                continue;
            }
            let path = store.includes[i].path.clone();
            if seen.contains(&path) {
                continue;
            }
            let t = std::time::Instant::now();
            let dir = crate::scan::scan_root(
                &path,
                &store.excludes,
                store.includes[i].one_file_system,
                cancel,
                progress,
            );
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return None;
            }
            seen.push(path);
            if let Some(dir) = dir {
                let (nf, nfi) = crate::scan::add_to_db(&mut store.db, dir);
                let inc = &mut store.includes[i];
                inc.last_scan_time = now;
                inc.last_scan_duration = t.elapsed().as_millis() as u32;
                inc.last_scanned_folder_count = nf;
                inc.last_scanned_file_count = nfi;
            }
        }
        store.build_sorted();
        Some(store)
    }

    pub fn ranks(&self) -> &Ranks {
        self.ranks.get_or_init(|| compute_ranks(&self.db))
    }

    /// Build the NAME, PATH, SIZE, MTIME and EXTENSION arrays.
    pub fn build_sorted(&mut self) {
        let ranks = compute_ranks(&self.db);
        let db = &self.db;
        let r = &ranks;
        let kinds = [false, true];
        let mut arrays: Vec<(Prop, bool, Vec<u32>)> = FAST_SORT
            .par_iter()
            .flat_map(|&p| kinds.par_iter().map(move |&is_file| (p, is_file)))
            .map(|(p, is_file)| (p, is_file, sorted_by(db, r, p, is_file)))
            .collect();
        let mut sorted: Vec<Option<Sorted>> = vec![None; NUM_PROPS];
        for (p, is_file, v) in arrays.drain(..) {
            let s = sorted[p as usize].get_or_insert_with(Sorted::default);
            if is_file {
                s.files = Arc::new(v);
            } else {
                s.folders = Arc::new(v);
            }
        }
        self.sorted = sorted;
        self.ranks = OnceLock::new();
        let _ = self.ranks.set(ranks);
    }

    pub fn types(&self) -> &TypeRanks {
        self.types.get_or_init(|| {
            let types: Vec<String> = self
                .db
                .files
                .par_iter()
                .map(|e| crate::mime::file_type(self.db.name_of(e), false))
                .collect();
            let mut names: Vec<String> = types.clone();
            names.sort();
            names.dedup();
            let index: std::collections::HashMap<&str, u32> = names
                .iter()
                .enumerate()
                .map(|(i, n)| (n.as_str(), i as u32))
                .collect();
            let file_type: Vec<u32> = types.iter().map(|t| index[t.as_str()]).collect();
            let rank = (0..names.len() as u32).collect();
            TypeRanks {
                names,
                file_type,
                rank,
            }
        })
    }

    /// Cached full paths of all folders, built on the first search that looks at paths.
    pub fn folder_paths(&self) -> &crate::db::FolderPaths {
        self.folder_paths
            .get_or_init(|| crate::db::FolderPaths::build(&self.db))
    }

    fn effective_sort(&self, sort: Prop) -> Prop {
        if self
            .sorted
            .get(sort as usize)
            .and_then(|s| s.as_ref())
            .is_none()
        {
            Prop::Name
        } else {
            sort
        }
    }

    /// `fsearch_database_index_store_search`.
    pub fn search(
        &self,
        query: &Query,
        sort: Prop,
        dir: SortDir,
        cancel: Option<&AtomicBool>,
    ) -> View {
        let sort = self.effective_sort(sort);
        let empty = Sorted::default();
        let arrays = self.sorted[sort as usize].as_ref().unwrap_or(&empty);
        let chain = chain_for(sort);
        if query.matches_everything() {
            return View {
                folders: arrays.folders.clone(),
                files: arrays.files.clone(),
                chain,
                dir,
                complete: true,
            };
        }
        self.filtered_view(query, &arrays.folders, &arrays.files, chain, dir, cancel)
    }

    /// Search as you type: when `query` can only match a subset of what `prev_query` matched (the
    /// new text extends the old one), filter the previous results instead of the whole index. The
    /// order is the same as a full search because both lists are filtered from the same sorted
    /// index. Falls back to [`Store::search`] otherwise.
    pub fn search_refine(
        &self,
        prev_query: &Query,
        prev: &View,
        query: &Query,
        sort: Prop,
        dir: SortDir,
        cancel: Option<&AtomicBool>,
    ) -> View {
        let chain = chain_for(self.effective_sort(sort));
        if prev.complete
            && prev.chain == chain
            && !query.matches_everything()
            && query.refines(prev_query)
        {
            return self.filtered_view(query, &prev.folders, &prev.files, chain, dir, cancel);
        }
        self.search(query, sort, dir, cancel)
    }

    fn filtered_view(
        &self,
        query: &Query,
        folders: &[u32],
        files: &[u32],
        chain: Vec<Prop>,
        dir: SortDir,
        cancel: Option<&AtomicBool>,
    ) -> View {
        let folders = self.filter(query, folders, true, cancel);
        let files = self.filter(query, files, false, cancel);
        let complete = cancel.is_none_or(|c| !c.load(std::sync::atomic::Ordering::Relaxed));
        View {
            folders: Arc::new(folders),
            files: Arc::new(files),
            chain,
            dir,
            complete,
        }
    }

    fn filter(
        &self,
        query: &Query,
        input: &[u32],
        folders: bool,
        cancel: Option<&AtomicBool>,
    ) -> Vec<u32> {
        let mk = |i: u32| if folders { Id::Folder(i) } else { Id::File(i) };
        let paths = query.uses_paths.then(|| self.folder_paths());
        let new_md = || match paths {
            Some(p) => MatchData::with_paths(&self.db, p),
            None => MatchData::new(&self.db),
        };
        let cancelled = || cancel.is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed));
        if input.len() < THRESHOLD_FOR_PARALLEL_SEARCH || query.wants_single_threaded_search {
            let mut md = new_md();
            let mut out = Vec::new();
            for &i in input {
                if cancelled() {
                    break;
                }
                md.set(mk(i));
                if query.matches(&mut md) {
                    out.push(i);
                }
            }
            return out;
        }
        let parts: Vec<Vec<u32>> = input
            .par_chunks(CHUNK)
            .map_init(new_md, |md, chunk| {
                let mut out = Vec::new();
                if cancelled() {
                    return out;
                }
                for &i in chunk {
                    md.set(mk(i));
                    if query.matches(md) {
                        out.push(i);
                    }
                }
                out
            })
            .collect();
        let total = parts.iter().map(Vec::len).sum();
        let mut out = Vec::with_capacity(total);
        for p in parts {
            out.extend_from_slice(&p);
        }
        out
    }

    /// `fsearch_database_index_store_sort_results` / `fsearch_database_sort_results`.
    pub fn sort_view(&self, view: &mut View, new: Prop, dir: SortDir, cancel: Option<&AtomicBool>) {
        let old = view.chain.first().copied().unwrap_or(Prop::None);
        if old == new {
            view.dir = dir;
            return;
        }
        if let Some(Some(index)) = self.sorted.get(new as usize) {
            view.folders = fast_sort(&view.folders, &index.folders, self.db.folders.len());
            view.files = fast_sort(&view.files, &index.files, self.db.files.len());
            view.chain = chain_for(new);
            view.dir = dir;
            return;
        }
        let chain = chain_prepend(&view.chain, new);
        let ranks = self.ranks();
        let types = if chain.contains(&Prop::FileType) {
            Some(self.types())
        } else {
            None
        };
        let affects_folders = !matches!(new, Prop::Extension | Prop::FileType);
        let mut folders = (*view.folders).clone();
        let mut files = (*view.files).clone();
        if affects_folders {
            folders.sort_by(|&a, &b| {
                compare_chain(&self.db, ranks, types, &chain, Id::Folder(a), Id::Folder(b))
            });
        }
        match (new, types) {
            // The view is already in `view.chain` order and the new chain is [type, old chain…]:
            // a stable sort by type alone yields exactly the chain order, without comparing
            // the whole chain for every pair (the original's merge sort is stable too).
            (Prop::FileType, Some(t)) => {
                files.par_sort_by_key(|&f| t.rank[t.file_type[f as usize] as usize]);
            }
            _ => files.sort_by(|&a, &b| {
                compare_chain(&self.db, ranks, types, &chain, Id::File(a), Id::File(b))
            }),
        }
        if cancel.is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed)) {
            return;
        }
        view.folders = Arc::new(folders);
        view.files = Arc::new(files);
        view.chain = chain;
        view.dir = dir;
    }
}

/// `fast_sort`: the whole index when nothing was filtered, else its entries in index order.
fn fast_sort(subset: &[u32], index: &Arc<Vec<u32>>, universe: usize) -> Arc<Vec<u32>> {
    if subset.len() == index.len() {
        return Arc::clone(index);
    }
    let mut mark = vec![false; universe];
    for &i in subset {
        mark[i as usize] = true;
    }
    let mut out = Vec::with_capacity(subset.len());
    for &i in index.iter() {
        if out.len() == subset.len() {
            break;
        }
        if mark[i as usize] {
            out.push(i);
        }
    }
    Arc::new(out)
}

fn compute_ranks(db: &Db) -> Ranks {
    let folder_path_rank = folder_path_ranks(db);
    let pk = |p: u32| {
        if p == NONE {
            0
        } else {
            folder_path_rank[p as usize] + 1
        }
    };
    let name_order = |entries: &[crate::db::Entry]| -> Vec<u32> {
        let mut v: Vec<u32> = (0..entries.len() as u32).collect();
        v.par_sort_by(|&a, &b| {
            let (ea, eb) = (&entries[a as usize], &entries[b as usize]);
            cmp_paths(db.name_of(ea), db.name_of(eb))
                .cmp(&0)
                .then_with(|| pk(ea.parent).cmp(&pk(eb.parent)))
        });
        v
    };
    let dense = |entries: &[crate::db::Entry], order: &[u32]| -> Vec<u32> {
        let mut rank = vec![0u32; entries.len()];
        let mut r = 0u32;
        let mut prev: Option<&[u8]> = None;
        for &i in order {
            let n = db.name_of(&entries[i as usize]);
            if let Some(p) = prev
                && p != n
            {
                r += 1;
            }
            prev = Some(n);
            rank[i as usize] = r;
        }
        rank
    };
    let (fo, fio) = rayon::join(|| name_order(&db.folders), || name_order(&db.files));
    Ranks {
        folder_name_rank: dense(&db.folders, &fo),
        file_name_rank: dense(&db.files, &fio),
        folder_path_rank,
    }
}

fn sorted_by(db: &Db, r: &Ranks, p: Prop, is_file: bool) -> Vec<u32> {
    let entries = if is_file { &db.files } else { &db.folders };
    let mk = |i: u32| if is_file { Id::File(i) } else { Id::Folder(i) };
    let mut v: Vec<u32> = (0..entries.len() as u32).collect();
    let name = |i: u32| r.name_key(mk(i));
    let parent = |i: u32| r.parent_key(db, mk(i));
    match p {
        Prop::Name => v.par_sort_unstable_by_key(|&i| (name(i), parent(i))),
        Prop::Path => v.par_sort_unstable_by_key(|&i| (parent(i), name(i))),
        Prop::Size => {
            v.par_sort_unstable_by_key(|&i| (entries[i as usize].size, name(i), parent(i)))
        }
        Prop::Mtime => {
            v.par_sort_unstable_by_key(|&i| (entries[i as usize].mtime, name(i), parent(i)))
        }
        Prop::Extension => {
            if is_file {
                v.par_sort_unstable_by(|&a, &b| {
                    let ea = db.extension(Id::File(a)).unwrap_or(b"");
                    let eb = db.extension(Id::File(b)).unwrap_or(b"");
                    ea.cmp(eb)
                        .then_with(|| (name(a), parent(a)).cmp(&(name(b), parent(b))))
                })
            } else {
                v.par_sort_unstable_by_key(|&i| (name(i), parent(i)))
            }
        }
        _ => {}
    }
    v
}

/// Compare full paths component by component (`db_entry_compare_entries_by_full_path`).
fn cmp_full_path(db: &Db, a: Id, b: Id) -> Ordering {
    let comps = |id: Id| {
        let mut v: Vec<&[u8]> = vec![db.name(id)];
        let mut p = db.entry(id).parent;
        while p != NONE {
            v.push(db.folder_name(p));
            p = db.folders[p as usize].parent;
        }
        v.reverse();
        v
    };
    let (ca, cb) = (comps(a), comps(b));
    for (x, y) in ca.iter().zip(cb.iter()) {
        let r = cmp_paths(x, y);
        if r != 0 {
            return r.cmp(&0);
        }
    }
    ca.len().cmp(&cb.len())
}

fn compare_chain(
    db: &Db,
    r: &Ranks,
    types: Option<&TypeRanks>,
    chain: &[Prop],
    a: Id,
    b: Id,
) -> Ordering {
    for &p in chain {
        let o = match p {
            Prop::Name => r.name_key(a).cmp(&r.name_key(b)),
            Prop::Path => r.parent_key(db, a).cmp(&r.parent_key(db, b)),
            Prop::PathFull => cmp_full_path(db, a, b),
            Prop::Size => db.entry(a).size.cmp(&db.entry(b).size),
            Prop::Mtime => db.entry(a).mtime.cmp(&db.entry(b).mtime),
            Prop::Extension => db
                .extension(a)
                .unwrap_or(b"")
                .cmp(db.extension(b).unwrap_or(b"")),
            Prop::FileType => match (types, a, b) {
                (Some(t), Id::File(x), Id::File(y)) => t.rank[t.file_type[x as usize] as usize]
                    .cmp(&t.rank[t.file_type[y as usize] as usize]),
                _ => Ordering::Equal,
            },
            _ => Ordering::Equal,
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    Ordering::Equal
}

/// A sorted result list: folders first, then files; descending reverses the whole list.
#[derive(Clone)]
pub struct View {
    pub folders: Arc<Vec<u32>>,
    pub files: Arc<Vec<u32>>,
    pub chain: Vec<Prop>,
    pub dir: SortDir,
    pub complete: bool,
}

impl View {
    pub fn len(&self) -> usize {
        self.folders.len() + self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn sort_prop(&self) -> Prop {
        self.chain.first().copied().unwrap_or(Prop::None)
    }

    /// `fsearch_database_search_view_get_entry_for_idx`.
    pub fn get(&self, idx: usize) -> Option<Id> {
        let n = self.len();
        if idx >= n {
            return None;
        }
        let i = if self.dir == SortDir::Descending {
            n - (idx + 1)
        } else {
            idx
        };
        if i < self.folders.len() {
            Some(Id::Folder(self.folders[i]))
        } else {
            Some(Id::File(self.files[i - self.folders.len()]))
        }
    }
}
