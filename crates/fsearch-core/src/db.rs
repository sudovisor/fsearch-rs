//! The entry arena: every folder and file of every index, with names stored once.
//!
//! Port of `FsearchDatabaseEntry` (`src/fsearch_database_entry.c`). Entries reference their parent
//! folder by index instead of by pointer; folders and files live in separate vectors like the
//! original's separate folder and file arrays.

use crate::cmp::cmp_paths;

pub const NONE: u32 = u32::MAX;

/// Index properties (`FsearchDatabaseIndexProperty`), numbered like the original: the numbers are
/// written to `fsearch.db` as sorted-array ids.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u32)]
pub enum Prop {
    None = 0,
    Name = 1,
    Path = 2,
    PathFull = 3,
    Size = 4,
    Mtime = 5,
    Atime = 6,
    Ctime = 7,
    StatusChangeTime = 8,
    NumFiles = 9,
    NumFolders = 10,
    FileType = 11,
    Extension = 12,
}

pub const NUM_PROPS: usize = 13;

impl Prop {
    pub fn from_u32(v: u32) -> Option<Prop> {
        use Prop::*;
        Some(match v {
            0 => None,
            1 => Name,
            2 => Path,
            3 => PathFull,
            4 => Size,
            5 => Mtime,
            6 => Atime,
            7 => Ctime,
            8 => StatusChangeTime,
            9 => NumFiles,
            10 => NumFolders,
            11 => FileType,
            12 => Extension,
            _ => return Option::None,
        })
    }
}

pub const FLAG_NAME: u64 = 1 << 0;
pub const FLAG_PATH: u64 = 1 << 1;
pub const FLAG_SIZE: u64 = 1 << 2;
pub const FLAG_MTIME: u64 = 1 << 3;
pub const FLAG_EXTENSION: u64 = 1 << 10;
pub const FLAG_DEFAULT: u64 = FLAG_NAME | FLAG_PATH | FLAG_SIZE | FLAG_MTIME;

/// Properties with a precomputed ("fast sort") index, in the order the file stores them.
pub const FAST_SORT: [Prop; 5] = [
    Prop::Name,
    Prop::Path,
    Prop::Size,
    Prop::Mtime,
    Prop::Extension,
];

#[derive(Clone, Copy, Debug, Default)]
pub struct Entry {
    pub name_off: u32,
    pub name_len: u16,
    pub parent: u32,
    pub size: i64,
    pub mtime: i64,
    pub num_files: u32,
    pub num_folders: u32,
}

/// An entry reference: a folder or a file index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Id {
    Folder(u32),
    File(u32),
}

impl Id {
    #[inline]
    pub fn is_folder(self) -> bool {
        matches!(self, Id::Folder(_))
    }
}

#[derive(Default, Clone)]
pub struct Db {
    pub names: Vec<u8>,
    pub folders: Vec<Entry>,
    pub files: Vec<Entry>,
}

impl Db {
    pub fn push_name(&mut self, name: &[u8]) -> (u32, u16) {
        let off = self.names.len() as u32;
        self.names.extend_from_slice(name);
        (off, name.len() as u16)
    }

    #[inline]
    pub fn entry(&self, id: Id) -> &Entry {
        match id {
            Id::Folder(i) => &self.folders[i as usize],
            Id::File(i) => &self.files[i as usize],
        }
    }

    #[inline]
    pub fn name_of(&self, e: &Entry) -> &[u8] {
        &self.names[e.name_off as usize..e.name_off as usize + e.name_len as usize]
    }

    #[inline]
    pub fn name(&self, id: Id) -> &[u8] {
        self.name_of(self.entry(id))
    }

    #[inline]
    pub fn folder_name(&self, i: u32) -> &[u8] {
        self.name_of(&self.folders[i as usize])
    }

    /// `db_entry_get_name_raw_for_display`: an empty name shows as `/`.
    pub fn display_name(&self, id: Id) -> &[u8] {
        let n = self.name(id);
        if n.is_empty() { b"/" } else { n }
    }

    /// `db_entry_get_extension`: `None` for folders.
    pub fn extension(&self, id: Id) -> Option<&[u8]> {
        match id {
            Id::Folder(_) => None,
            Id::File(_) => Some(fsearch_string_utils_rs::port::get_extension(self.name(id))),
        }
    }

    pub fn depth(&self, id: Id) -> u32 {
        let mut d = 0;
        let mut p = self.entry(id).parent;
        while p != NONE {
            d += 1;
            p = self.folders[p as usize].parent;
        }
        d
    }

    fn build_folder_path(&self, folder: u32, out: &mut Vec<u8>) {
        // collect ancestors iteratively (root first)
        let mut chain: [u32; 64] = [0; 64];
        let mut overflow: Vec<u32> = Vec::new();
        let mut n = 0usize;
        let mut f = folder;
        while f != NONE {
            if n < chain.len() {
                chain[n] = f;
            } else {
                overflow.push(f);
            }
            n += 1;
            f = self.folders[f as usize].parent;
        }
        for k in (0..n).rev() {
            let fi = if k < chain.len() {
                chain[k]
            } else {
                overflow[k - chain.len()]
            };
            let name = self.folder_name(fi);
            if !name.is_empty() && name != b"/" {
                out.extend_from_slice(name);
            }
            out.push(b'/');
        }
    }

    /// `db_entry_append_path`: the parent's path, without a trailing separator.
    pub fn append_parent_path(&self, id: Id, out: &mut Vec<u8>) {
        let start = out.len();
        let p = self.entry(id).parent;
        if p != NONE {
            self.build_folder_path(p, out);
        }
        if out.len() - start > 1 {
            out.pop();
        }
    }

    /// `db_entry_append_full_path`.
    pub fn append_full_path(&self, id: Id, out: &mut Vec<u8>) {
        let e = self.entry(id);
        if e.parent != NONE {
            self.build_folder_path(e.parent, out);
        }
        if e.name_len > 0 {
            out.extend_from_slice(self.name_of(e));
        } else {
            out.push(b'/');
        }
    }

    pub fn full_path(&self, id: Id) -> Vec<u8> {
        let mut v = Vec::with_capacity(128);
        self.append_full_path(id, &mut v);
        v
    }

    pub fn parent_path(&self, id: Id) -> Vec<u8> {
        let mut v = Vec::with_capacity(128);
        self.append_parent_path(id, &mut v);
        v
    }

    /// Root folder name of an entry (`db_entry_get_root_path`).
    pub fn root_name(&self, id: Id) -> &[u8] {
        let mut cur = id;
        loop {
            let p = self.entry(cur).parent;
            if p == NONE {
                return self.name(cur);
            }
            cur = Id::Folder(p);
        }
    }

    /// `db_entry_set_parent` on a fresh entry: bump child counts and add its size to every
    /// ancestor folder.
    pub fn link_with_size(&mut self, id: Id, parent: u32) {
        if parent == NONE {
            return;
        }
        let size = self.entry(id).size;
        {
            let p = &mut self.folders[parent as usize];
            if id.is_folder() {
                p.num_folders += 1;
            } else {
                p.num_files += 1;
            }
        }
        let mut f = parent;
        while f != NONE {
            let pe = &mut self.folders[f as usize];
            if size < 0 && pe.size + size < 0 {
                pe.size = 0;
            } else {
                pe.size += size;
            }
            f = pe.parent;
        }
    }
}

/// Ranks that turn every comparator of the original into integer comparisons.
///
/// - `folder_path_rank[f]`: position of folder `f` in the component-wise ordering of full paths
///   (preorder of the folder tree, children ordered by `cmp_paths`). The original compares parent
///   chains component by component with a shorter prefix first, which is exactly this order.
/// - `*_name_rank`: dense rank of the name under `cmp_paths` (equal names share a rank).
#[derive(Default, Clone)]
pub struct Ranks {
    pub folder_path_rank: Vec<u32>,
    pub folder_name_rank: Vec<u32>,
    pub file_name_rank: Vec<u32>,
}

impl Ranks {
    /// PATH comparison key of an entry: its parent's path rank (+1), 0 for roots.
    #[inline]
    pub fn parent_key(&self, db: &Db, id: Id) -> u32 {
        let p = db.entry(id).parent;
        if p == NONE {
            0
        } else {
            self.folder_path_rank[p as usize] + 1
        }
    }

    #[inline]
    pub fn name_key(&self, id: Id) -> u32 {
        match id {
            Id::Folder(i) => self.folder_name_rank[i as usize],
            Id::File(i) => self.file_name_rank[i as usize],
        }
    }
}

/// Preorder ranks of all folders: roots by `cmp_paths` of their names, children likewise.
pub fn folder_path_ranks(db: &Db) -> Vec<u32> {
    use rayon::prelude::*;
    let n = db.folders.len();
    let mut child_count = vec![0u32; n + 1];
    for f in &db.folders {
        let slot = if f.parent == NONE {
            n
        } else {
            f.parent as usize
        };
        child_count[slot] += 1;
    }
    let mut start = vec![0usize; n + 2];
    for i in 0..=n {
        start[i + 1] = start[i] + child_count[i] as usize;
    }
    let mut fill = start.clone();
    let mut children = vec![0u32; n];
    for (i, f) in db.folders.iter().enumerate() {
        let slot = if f.parent == NONE {
            n
        } else {
            f.parent as usize
        };
        children[fill[slot]] = i as u32;
        fill[slot] += 1;
    }
    // sort every child list by name (in parallel over the lists)
    {
        let mut slices: Vec<&mut [u32]> = Vec::with_capacity(n + 1);
        let mut rest: &mut [u32] = &mut children;
        for i in 0..=n {
            let (head, tail) = rest.split_at_mut(start[i + 1] - start[i]);
            slices.push(head);
            rest = tail;
        }
        slices.par_iter_mut().filter(|s| s.len() > 1).for_each(|s| {
            s.sort_by(|&a, &b| cmp_paths(db.folder_name(a), db.folder_name(b)).cmp(&0))
        });
    }
    let mut rank = vec![0u32; n];
    let mut next = 0u32;
    let mut stack: Vec<(usize, usize)> = vec![(start[n], start[n + 1])];
    while let Some(top) = stack.last_mut() {
        if top.0 == top.1 {
            stack.pop();
            continue;
        }
        let f = children[top.0] as usize;
        top.0 += 1;
        rank[f] = next;
        next += 1;
        stack.push((start[f], start[f + 1]));
    }
    rank
}

/// Every folder's path with a trailing separator (what `build_path_recursively` produces), built
/// once so path searches copy one slice instead of walking the parent chain for every entry.
pub struct FolderPaths {
    buf: Vec<u8>,
    off: Vec<u64>,
}

impl FolderPaths {
    pub fn build(db: &Db) -> FolderPaths {
        let n = db.folders.len();
        let mut start = vec![u64::MAX; n];
        let mut len = vec![0u32; n];
        let mut buf: Vec<u8> = Vec::new();
        let mut chain: Vec<u32> = Vec::new();
        for f in 0..n as u32 {
            if start[f as usize] != u64::MAX {
                continue;
            }
            chain.clear();
            let mut cur = f;
            while cur != NONE && start[cur as usize] == u64::MAX {
                chain.push(cur);
                cur = db.folders[cur as usize].parent;
            }
            // `cur` is NONE or a folder that is already done; build downwards from it
            for &g in chain.iter().rev() {
                let p = db.folders[g as usize].parent;
                let s = buf.len() as u64;
                if p != NONE {
                    let (ps, pl) = (start[p as usize] as usize, len[p as usize] as usize);
                    buf.extend_from_within(ps..ps + pl);
                }
                let name = db.folder_name(g);
                if !name.is_empty() && name != b"/" {
                    buf.extend_from_slice(name);
                }
                buf.push(b'/');
                start[g as usize] = s;
                len[g as usize] = (buf.len() as u64 - s) as u32;
            }
        }
        let mut off = Vec::with_capacity(n + 1);
        for i in 0..n {
            off.push(start[i]);
            off.push(start[i] + len[i] as u64);
        }
        FolderPaths { buf, off }
    }

    #[inline]
    fn folder(&self, f: u32) -> &[u8] {
        let i = 2 * f as usize;
        &self.buf[self.off[i] as usize..self.off[i + 1] as usize]
    }

    /// Same bytes as `Db::append_full_path`.
    pub fn append_full_path(&self, db: &Db, id: Id, out: &mut Vec<u8>) {
        let e = db.entry(id);
        if e.parent != NONE {
            out.extend_from_slice(self.folder(e.parent));
        }
        if e.name_len > 0 {
            out.extend_from_slice(db.name_of(e));
        } else {
            out.push(b'/');
        }
    }

    /// Same bytes as `Db::append_parent_path`.
    pub fn append_parent_path(&self, db: &Db, id: Id, out: &mut Vec<u8>) {
        let start = out.len();
        let p = db.entry(id).parent;
        if p != NONE {
            out.extend_from_slice(self.folder(p));
        }
        if out.len() - start > 1 {
            out.pop();
        }
    }
}
