//! Port of `db_scan_folder` (`src/fsearch_database_scan.c`).
//!
//! The original walks one directory at a time on one thread. This walks sibling directories in
//! parallel (rayon) and then flattens the tree into the arena; the database sorts every array
//! afterwards, so the result is the same set of entries with the same sizes and counts.

use crate::db::{Db, Entry, Id, NONE};
use crate::rules::ExcludeManager;
use rayon::prelude::*;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub struct ScanCtx<'a> {
    pub excludes: &'a ExcludeManager,
    pub one_file_system: bool,
    pub root_dev: u64,
    pub cancel: &'a AtomicBool,
    pub progress: &'a AtomicU64,
}

struct FileRec {
    name_off: u32,
    name_len: u16,
    size: i64,
    mtime: i64,
}

pub struct Dir {
    name: Vec<u8>,
    mtime: i64,
    names: Vec<u8>,
    files: Vec<FileRec>,
    dirs: Vec<Dir>,
}

#[cfg(unix)]
mod sys {
    use super::*;
    use rustix::fd::{AsFd, OwnedFd};
    use rustix::fs::{AtFlags, FileType, Mode, OFlags};

    #[cfg(target_os = "macos")]
    fn device_id(device: i32) -> u64 {
        // Darwin's signed dev_t still identifies the same device by its 32-bit value.
        u64::from(device as u32)
    }

    #[cfg(not(target_os = "macos"))]
    fn device_id(device: impl Into<u64>) -> u64 {
        device.into()
    }

    fn open_dir(path: &[u8]) -> Option<OwnedFd> {
        let p = if path.is_empty() {
            b"/".as_slice()
        } else {
            path
        };
        rustix::fs::open(
            p,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOCTTY,
            Mode::empty(),
        )
        .ok()
    }

    /// Scan the directory at `path` (which is the folder `name`), recursing in parallel.
    fn scan_dir(ctx: &ScanCtx, path: &[u8], name: Vec<u8>, mtime: i64) -> Dir {
        let mut dir = Dir {
            name,
            mtime,
            names: Vec::new(),
            files: Vec::new(),
            dirs: Vec::new(),
        };
        if ctx.cancel.load(Ordering::Relaxed) {
            return dir;
        }
        let Some(fd) = open_dir(path) else {
            return dir;
        };
        ctx.progress.fetch_add(1, Ordering::Relaxed);
        let mut sub: Vec<(Vec<u8>, Vec<u8>, i64)> = Vec::new();
        let Ok(mut rd) = rustix::fs::Dir::read_from(&fd) else {
            return dir;
        };
        let mut child_path = Vec::with_capacity(path.len() + 64);
        while let Some(Ok(ent)) = rd.next() {
            if ctx.cancel.load(Ordering::Relaxed) {
                break;
            }
            let fname = ent.file_name().to_bytes();
            if fname == b"." || fname == b".." {
                continue;
            }
            if fname.len() > u16::MAX as usize {
                continue;
            }
            child_path.clear();
            child_path.extend_from_slice(path);
            child_path.push(b'/');
            child_path.extend_from_slice(fname);
            #[cfg(target_os = "linux")]
            let at_flags = AtFlags::SYMLINK_NOFOLLOW | AtFlags::NO_AUTOMOUNT;
            #[cfg(not(target_os = "linux"))]
            let at_flags = AtFlags::SYMLINK_NOFOLLOW;
            let Ok(st) = rustix::fs::statat(fd.as_fd(), ent.file_name(), at_flags) else {
                continue;
            };
            if ctx.one_file_system && device_id(st.st_dev) != ctx.root_dev {
                continue;
            }
            let is_dir = FileType::from_raw_mode(st.st_mode) == FileType::Directory;
            if ctx.excludes.excludes(&child_path, fname, is_dir) {
                continue;
            }
            if is_dir {
                sub.push((child_path.clone(), fname.to_vec(), st.st_mtime));
            } else {
                let off = dir.names.len() as u32;
                dir.names.extend_from_slice(fname);
                dir.files.push(FileRec {
                    name_off: off,
                    name_len: fname.len() as u16,
                    size: st.st_size,
                    mtime: st.st_mtime,
                });
            }
        }
        drop(rd);
        drop(fd);
        dir.dirs = if sub.len() > 1 {
            sub.into_par_iter()
                .map(|(p, n, m)| scan_dir(ctx, &p, n, m))
                .collect()
        } else {
            sub.into_iter()
                .map(|(p, n, m)| scan_dir(ctx, &p, n, m))
                .collect()
        };
        dir
    }

    /// Scan one include root. Returns `None` when the root is missing or unreadable (the original's
    /// `db_scan_folder` failing), so the index stays empty.
    pub fn scan_root(
        root: &str,
        excludes: &ExcludeManager,
        one_file_system: bool,
        cancel: &AtomicBool,
        progress: &AtomicU64,
    ) -> Option<Dir> {
        // g_file_test(IS_DIR) follows symlinks; lstat gives the mtime and device of the root itself.
        let is_dir = std::fs::metadata(root).map(|m| m.is_dir()).unwrap_or(false);
        if !is_dir {
            return None;
        }
        let st = rustix::fs::lstat(root).ok()?;
        let path: Vec<u8> = if root == "/" {
            Vec::new()
        } else {
            root.as_bytes().to_vec()
        };
        // the original fails the whole scan when the root itself cannot be opened
        open_dir(&path)?;
        let ctx = ScanCtx {
            excludes,
            one_file_system,
            root_dev: device_id(st.st_dev),
            cancel,
            progress,
        };
        let d = scan_dir(&ctx, &path, root.as_bytes().to_vec(), st.st_mtime);
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        Some(d)
    }
}

/// Windows (and other non-Unix systems): `std::fs`, paths kept with `/` separators so queries
/// and the stored format look the same on every system (ADR-022).
#[cfg(not(unix))]
mod sys {
    use super::*;
    use std::time::UNIX_EPOCH;

    fn secs(t: std::io::Result<std::time::SystemTime>) -> i64 {
        match t {
            Ok(t) => match t.duration_since(UNIX_EPOCH) {
                Ok(d) => d.as_secs() as i64,
                Err(e) => -(e.duration().as_secs() as i64),
            },
            Err(_) => 0,
        }
    }

    /// `C:` reads as the current directory of drive C; the folder itself is `C:/`.
    fn fs_path(path: &[u8]) -> String {
        let s = String::from_utf8_lossy(path).into_owned();
        if s.ends_with(':') { s + "/" } else { s }
    }

    fn scan_dir(ctx: &ScanCtx, path: &[u8], name: Vec<u8>, mtime: i64) -> Dir {
        let mut dir = Dir {
            name,
            mtime,
            names: Vec::new(),
            files: Vec::new(),
            dirs: Vec::new(),
        };
        if ctx.cancel.load(Ordering::Relaxed) {
            return dir;
        }
        let Ok(rd) = std::fs::read_dir(fs_path(path)) else {
            return dir;
        };
        ctx.progress.fetch_add(1, Ordering::Relaxed);
        let mut sub: Vec<(Vec<u8>, Vec<u8>, i64)> = Vec::new();
        let mut child_path = Vec::with_capacity(path.len() + 64);
        for ent in rd.map_while(Result::ok) {
            if ctx.cancel.load(Ordering::Relaxed) {
                break;
            }
            let fname = ent.file_name().to_string_lossy().into_owned().into_bytes();
            if fname.len() > u16::MAX as usize {
                continue;
            }
            child_path.clear();
            child_path.extend_from_slice(path);
            child_path.push(b'/');
            child_path.extend_from_slice(&fname);
            // DirEntry::metadata does not follow symlinks / junctions
            let Ok(meta) = ent.metadata() else { continue };
            let is_dir = meta.is_dir() && !meta.file_type().is_symlink();
            if ctx.excludes.excludes(&child_path, &fname, is_dir) {
                continue;
            }
            let m = secs(meta.modified());
            if is_dir {
                sub.push((child_path.clone(), fname, m));
            } else {
                let off = dir.names.len() as u32;
                dir.names.extend_from_slice(&fname);
                dir.files.push(FileRec {
                    name_off: off,
                    name_len: fname.len() as u16,
                    size: meta.len() as i64,
                    mtime: m,
                });
            }
        }
        dir.dirs = if sub.len() > 1 {
            sub.into_par_iter()
                .map(|(p, n, m)| scan_dir(ctx, &p, n, m))
                .collect()
        } else {
            sub.into_iter()
                .map(|(p, n, m)| scan_dir(ctx, &p, n, m))
                .collect()
        };
        dir
    }

    pub fn scan_root(
        root: &str,
        excludes: &ExcludeManager,
        one_file_system: bool,
        cancel: &AtomicBool,
        progress: &AtomicU64,
    ) -> Option<Dir> {
        let norm = root.replace('\\', "/");
        let name = if norm.len() > 1 && norm.ends_with('/') {
            norm.trim_end_matches('/').to_string()
        } else {
            norm
        };
        let meta = std::fs::metadata(fs_path(name.as_bytes())).ok()?;
        if !meta.is_dir() {
            return None;
        }
        std::fs::read_dir(fs_path(name.as_bytes())).ok()?;
        let ctx = ScanCtx {
            excludes,
            one_file_system,
            root_dev: 0,
            cancel,
            progress,
        };
        let d = scan_dir(
            &ctx,
            name.as_bytes(),
            name.as_bytes().to_vec(),
            secs(meta.modified()),
        );
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        Some(d)
    }
}

pub use sys::scan_root;

/// Add a scanned tree to the arena. Folder sizes are the sum of everything below them, child
/// counts are direct children (`db_entry_set_parent` on every added entry). Returns `(folders, files)`.
pub fn add_to_db(db: &mut Db, root: Dir) -> (u32, u32) {
    let (f0, fi0) = (db.folders.len(), db.files.len());
    // parents are always pushed before their children
    let mut stack: Vec<(Dir, u32)> = vec![(root, NONE)];
    while let Some((dir, parent)) = stack.pop() {
        let (off, len) = db.push_name(&dir.name);
        let idx = db.folders.len() as u32;
        let mut size = 0i64;
        let base = db.names.len() as u32;
        db.names.extend_from_slice(&dir.names);
        for f in &dir.files {
            size += f.size;
            db.files.push(Entry {
                name_off: base + f.name_off,
                name_len: f.name_len,
                parent: idx,
                size: f.size,
                mtime: f.mtime,
                ..Default::default()
            });
        }
        db.folders.push(Entry {
            name_off: off,
            name_len: len,
            parent,
            size,
            mtime: dir.mtime,
            num_files: dir.files.len() as u32,
            num_folders: dir.dirs.len() as u32,
        });
        for d in dir.dirs {
            stack.push((d, idx));
        }
    }
    for f in (f0..db.folders.len()).rev() {
        let (p, s) = (db.folders[f].parent, db.folders[f].size);
        if p != NONE {
            db.folders[p as usize].size += s;
        }
    }
    let _ = Id::Folder(0);
    (
        (db.folders.len() - f0) as u32,
        (db.files.len() - fi0) as u32,
    )
}
