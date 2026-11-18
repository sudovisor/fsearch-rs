//! Port of `src/fsearch_database_file.c`: the `fsearch.db` format, version 6.0, byte for byte.
//!
//! Layout (little-endian host): magic `FSDB`, major 6, minor 0, endianness flag, index flags (u64),
//! fast-sort flags (u64), includes, excludes, folder count, file count, block sizes, an MD5 over
//! everything before it, the folder block, the file block (names prefix-compressed against the
//! previous entry), then the fast-sort arrays as positions into the name-sorted lists.

use crate::db::{
    Db, Entry, FLAG_EXTENSION, FLAG_MTIME, FLAG_NAME, FLAG_PATH, FLAG_SIZE, NONE, NUM_PROPS, Prop,
};
use crate::rules::{Exclude, ExcludeManager, ExcludeType, Include, Scope, Target};
use crate::store::{Sorted, Store};
use md5::{Digest, Md5};
use std::io::Write;

const MAGIC: &[u8; 4] = b"FSDB";
const MAJOR: u8 = 6;
const MINOR: u8 = 0;
const PATH_MAX: usize = 4096;

#[derive(Debug)]
pub enum LoadError {
    Io,
    Locked,
    Format(&'static str),
    ConfigChanged,
}

fn open_locked(path: &std::path::Path, write: bool) -> Result<std::fs::File, LoadError> {
    let f = if write {
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
    } else {
        std::fs::File::open(path)
    }
    .map_err(|_| LoadError::Io)?;
    // exclusive and non-blocking, like the original's flock(LOCK_EX | LOCK_NB)
    f.try_lock().map_err(|_| LoadError::Locked)?;
    Ok(f)
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], LoadError> {
        if self.pos + n > self.buf.len() {
            return Err(LoadError::Format("truncated"));
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, LoadError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, LoadError> {
        Ok(u16::from_le_bytes(
            self.take(2)?
                .try_into()
                .map_err(|_| LoadError::Format("u16"))?,
        ))
    }
    fn u32(&mut self) -> Result<u32, LoadError> {
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| LoadError::Format("u32"))?,
        ))
    }
    fn u64(&mut self) -> Result<u64, LoadError> {
        Ok(u64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| LoadError::Format("u64"))?,
        ))
    }
    fn string(&mut self, max: usize) -> Result<String, LoadError> {
        let len = self.u32()? as usize;
        if len > max {
            return Err(LoadError::Format("string too long"));
        }
        Ok(String::from_utf8_lossy(self.take(len)?).into_owned())
    }
}

fn read_includes(r: &mut Reader) -> Result<Vec<Include>, LoadError> {
    let n = r.u32()?;
    let mut v: Vec<Include> = Vec::new();
    for _ in 0..n {
        let _type = r.u32()?;
        let path = r.string(4 * PATH_MAX)?;
        let one_fs = r.u8()? != 0;
        let active = r.u8()? != 0;
        let monitored = r.u8()? != 0;
        let scan_after_launch = r.u8()? != 0;
        let last_scan_time = r.u64()? as i64;
        let last_scan_duration = r.u32()?;
        let rescan_after = r.u64()? as i64;
        let last_error_code = r.u32()?;
        let last_scanned_folder_count = r.u32()?;
        let last_scanned_file_count = r.u32()?;
        let last_scan_reason = r.u8()?;
        let mut inc = Include::new(
            &path,
            active,
            one_fs,
            monitored,
            scan_after_launch,
            rescan_after,
        );
        inc.last_scan_time = last_scan_time;
        inc.last_scan_duration = last_scan_duration;
        inc.last_error_code = last_error_code;
        inc.last_scanned_folder_count = last_scanned_folder_count;
        inc.last_scanned_file_count = last_scanned_file_count;
        inc.last_scan_reason = last_scan_reason;
        crate::rules::add_include(&mut v, inc);
    }
    Ok(v)
}

fn read_excludes(r: &mut Reader) -> Result<ExcludeManager, LoadError> {
    let n = r.u32()?;
    let mut m = ExcludeManager::default();
    for _ in 0..n {
        let _record = r.u32()?;
        let kind = ExcludeType::from_u8(r.u8()?);
        let scope = Scope::from_u8(r.u8()?);
        let target = Target::from_u8(r.u8()?);
        let pattern = r.string(4 * PATH_MAX)?;
        let active = r.u8()? != 0;
        m.add(Exclude::new(&pattern, active, kind, scope, target));
    }
    m.exclude_hidden = r.u8()? != 0;
    Ok(m)
}

struct Header {
    flags: u64,
    includes: Vec<Include>,
    excludes: ExcludeManager,
    num_folders: u32,
    num_files: u32,
    folder_block: u64,
    file_block: u64,
}

fn read_header(r: &mut Reader) -> Result<Header, LoadError> {
    let start = r.pos;
    if r.take(4)? != MAGIC {
        return Err(LoadError::Format("magic"));
    }
    if r.u8()? != MAJOR {
        return Err(LoadError::Format("major version"));
    }
    if r.u8()? > MINOR {
        return Err(LoadError::Format("minor version"));
    }
    if r.u8()? != cfg!(target_endian = "little") as u8 {
        return Err(LoadError::Format("endianness"));
    }
    let flags = r.u64()?;
    let _fast_sort_flags = r.u64()?;
    let includes = read_includes(r)?;
    let excludes = read_excludes(r)?;
    let num_folders = r.u32()?;
    let num_files = r.u32()?;
    let folder_block = r.u64()?;
    let file_block = r.u64()?;
    let digest = Md5::digest(&r.buf[start..r.pos]);
    if r.take(16)? != digest.as_slice() {
        return Err(LoadError::Format("checksum"));
    }
    Ok(Header {
        flags,
        includes,
        excludes,
        num_folders,
        num_files,
        folder_block,
        file_block,
    })
}

/// `fsearch_database_file_load_config`: only the include/exclude configuration and flags.
pub fn load_config(
    path: &std::path::Path,
) -> Result<(Vec<Include>, ExcludeManager, u64), LoadError> {
    let f = open_locked(path, false)?;
    let data = std::fs::read(path).map_err(|_| LoadError::Io)?;
    drop(f);
    let mut r = Reader { buf: &data, pos: 0 };
    let h = read_header(&mut r)?;
    Ok((h.includes, h.excludes, h.flags))
}

fn read_block(
    db: &mut Db,
    block: &[u8],
    count: u32,
    flags: u64,
    folders: bool,
    num_folders: u32,
) -> Result<Vec<Entry>, LoadError> {
    // Counts come from the file: every entry takes at least 2+2+4 bytes (+8 per stored
    // attribute), so a count the block cannot hold is corruption, not an allocation size.
    let min_entry =
        8 + 8 * ((flags & FLAG_SIZE != 0) as usize + (flags & FLAG_MTIME != 0) as usize);
    if (count as usize).saturating_mul(min_entry) > block.len() {
        return Err(LoadError::Format("entry count exceeds block"));
    }
    let mut r = Reader { buf: block, pos: 0 };
    let mut prev: Vec<u8> = Vec::with_capacity(256);
    let mut out = Vec::with_capacity(count as usize);
    for idx in 0..count {
        let offset = r.u16()? as usize;
        let len = r.u16()? as usize;
        let suffix = r.take(len)?;
        prev.truncate(offset.min(prev.len()));
        prev.extend_from_slice(suffix);
        if prev.len() > u16::MAX as usize {
            return Err(LoadError::Format("name too long"));
        }
        let (name_off, name_len) = db.push_name(&prev);
        let mut e = Entry {
            name_off,
            name_len,
            parent: NONE,
            ..Default::default()
        };
        if flags & FLAG_SIZE != 0 {
            e.size = r.u64()? as i64;
        }
        if flags & FLAG_MTIME != 0 {
            e.mtime = r.u64()? as i64;
        }
        let parent = r.u32()?;
        if parent != u32::MAX && parent >= num_folders {
            return Err(LoadError::Format("parent index"));
        }
        e.parent = if folders && parent == idx {
            NONE
        } else {
            parent
        };
        out.push(e);
    }
    if r.pos != block.len() {
        return Err(LoadError::Format("block size"));
    }
    Ok(out)
}

/// `fsearch_database_file_load`. With `expect`, the stored include/exclude configuration must
/// equal it (otherwise the database is stale and the caller rescans).
pub fn load(
    path: &std::path::Path,
    expect: Option<(&[Include], &ExcludeManager)>,
) -> Result<Store, LoadError> {
    let f = open_locked(path, false)?;
    let data = std::fs::read(path).map_err(|_| LoadError::Io)?;
    let mut r = Reader { buf: &data, pos: 0 };
    let h = read_header(&mut r)?;
    if let Some((inc, exc)) = expect {
        let same_inc = inc.len() == h.includes.len()
            && inc.iter().zip(&h.includes).all(|(a, b)| a.same_config(b));
        if !same_inc || *exc != h.excludes {
            return Err(LoadError::ConfigChanged);
        }
    }
    let mut db = Db {
        names: Vec::with_capacity(data.len()),
        ..Default::default()
    };
    let fb = r.take(h.folder_block as usize)?;
    db.folders = read_block(&mut db, fb, h.num_folders, h.flags, true, h.num_folders)?;
    for i in 0..db.folders.len() {
        let p = db.folders[i].parent;
        if p != NONE {
            db.folders[p as usize].num_folders += 1;
        }
    }
    let fib = r.take(h.file_block as usize)?;
    db.files = read_block(&mut db, fib, h.num_files, h.flags, false, h.num_folders)?;
    for i in 0..db.files.len() {
        let p = db.files[i].parent;
        if p != NONE && (p as usize) < db.folders.len() {
            db.folders[p as usize].num_files += 1;
        }
    }
    check_folder_tree(&db)?;
    let n_sorted = r.u32()?;
    let mut sorted: Vec<Option<Sorted>> = vec![None; NUM_PROPS];
    let read_idx = |r: &mut Reader, n: u32| -> Result<Vec<u32>, LoadError> {
        let bytes = r.take(4 * n as usize)?;
        let v: Vec<u32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
            .collect();
        if v.iter().any(|&i| i >= n) {
            return Err(LoadError::Format("sorted index"));
        }
        Ok(v)
    };
    for _ in 0..n_sorted {
        let id = r.u32()?;
        if !(1..NUM_PROPS as u32).contains(&id) {
            return Err(LoadError::Format("sorted array id"));
        }
        let folders = read_idx(&mut r, h.num_folders)?;
        let files = read_idx(&mut r, h.num_files)?;
        sorted[id as usize] = Some(Sorted {
            folders: std::sync::Arc::new(folders),
            files: std::sync::Arc::new(files),
        });
    }
    drop(f);
    db.names.shrink_to_fit();
    Ok(Store::from_parts(
        db, h.includes, h.excludes, h.flags, sorted,
    ))
}

/// Every folder must reach a root: parent indices that form a cycle would make path building
/// loop forever (the original has the same flaw).
fn check_folder_tree(db: &Db) -> Result<(), LoadError> {
    // 0 = unvisited, 1 = on the current chain, 2 = reaches a root
    let mut state = vec![0u8; db.folders.len()];
    let mut chain: Vec<u32> = Vec::new();
    for f in 0..db.folders.len() as u32 {
        chain.clear();
        let mut cur = f;
        loop {
            if cur == NONE || state[cur as usize] == 2 {
                break;
            }
            if state[cur as usize] == 1 {
                return Err(LoadError::Format("folder parents form a cycle"));
            }
            state[cur as usize] = 1;
            chain.push(cur);
            cur = db.folders[cur as usize].parent;
        }
        for &c in &chain {
            state[c as usize] = 2;
        }
    }
    Ok(())
}

fn common_prefix(a: &[u8], b: &[u8]) -> usize {
    // the original compares C strings: a NUL in either ends the prefix
    let mut i = 0;
    while i < u16::MAX as usize && i < a.len() && i < b.len() && a[i] == b[i] && a[i] != 0 {
        i += 1;
    }
    i
}

fn write_entries(
    out: &mut Vec<u8>,
    db: &Db,
    order: &[u32],
    folders: bool,
    flags: u64,
    folder_pos: &[u32],
) {
    let mut prev: Vec<u8> = Vec::new();
    for (i, &idx) in order.iter().enumerate() {
        let e = if folders {
            &db.folders[idx as usize]
        } else {
            &db.files[idx as usize]
        };
        let name = db.name_of(e);
        let off = common_prefix(&prev, name);
        let len = name.len() - off;
        out.extend_from_slice(&(off as u16).to_le_bytes());
        out.extend_from_slice(&(len as u16).to_le_bytes());
        out.extend_from_slice(&name[off..]);
        prev.clear();
        prev.extend_from_slice(name);
        if flags & FLAG_SIZE != 0 {
            out.extend_from_slice(&(e.size as u64).to_le_bytes());
        }
        if flags & FLAG_MTIME != 0 {
            out.extend_from_slice(&(e.mtime as u64).to_le_bytes());
        }
        let parent = if e.parent == NONE {
            if folders { i as u32 } else { 0 }
        } else {
            folder_pos[e.parent as usize]
        };
        out.extend_from_slice(&parent.to_le_bytes());
    }
}

/// `fsearch_database_file_save`: written to `<path>.tmp` under an exclusive lock, then renamed.
pub fn save(store: &Store, path: &std::path::Path) -> std::io::Result<()> {
    let empty = Sorted::default();
    let names = store.sorted[Prop::Name as usize].as_ref().unwrap_or(&empty);
    let db = &store.db;
    let mut folder_pos = vec![0u32; db.folders.len()];
    for (pos, &f) in names.folders.iter().enumerate() {
        folder_pos[f as usize] = pos as u32;
    }
    let mut file_pos = vec![0u32; db.files.len()];
    for (pos, &f) in names.files.iter().enumerate() {
        file_pos[f as usize] = pos as u32;
    }

    let mut head: Vec<u8> = Vec::with_capacity(4096);
    head.extend_from_slice(MAGIC);
    head.push(MAJOR);
    head.push(MINOR);
    head.push(cfg!(target_endian = "little") as u8);
    head.extend_from_slice(&store.flags.to_le_bytes());
    let fast = FLAG_NAME | FLAG_PATH | FLAG_SIZE | FLAG_MTIME | FLAG_EXTENSION;
    head.extend_from_slice(&fast.to_le_bytes());
    head.extend_from_slice(&(store.includes.len() as u32).to_le_bytes());
    for inc in &store.includes {
        head.extend_from_slice(&0u32.to_le_bytes());
        head.extend_from_slice(&(inc.path.len() as u32).to_le_bytes());
        head.extend_from_slice(inc.path.as_bytes());
        head.push(inc.one_file_system as u8);
        head.push(inc.active as u8);
        head.push(inc.monitored as u8);
        head.push(inc.scan_after_launch as u8);
        head.extend_from_slice(&inc.last_scan_time.to_le_bytes());
        head.extend_from_slice(&inc.last_scan_duration.to_le_bytes());
        head.extend_from_slice(&inc.rescan_after.to_le_bytes());
        head.extend_from_slice(&inc.last_error_code.to_le_bytes());
        head.extend_from_slice(&inc.last_scanned_folder_count.to_le_bytes());
        head.extend_from_slice(&inc.last_scanned_file_count.to_le_bytes());
        head.push(inc.last_scan_reason);
    }
    head.extend_from_slice(&(store.excludes.excludes.len() as u32).to_le_bytes());
    for e in &store.excludes.excludes {
        head.extend_from_slice(&0u32.to_le_bytes());
        head.push(e.kind as u8);
        head.push(e.scope as u8);
        head.push(e.target as u8);
        head.extend_from_slice(&(e.pattern.len() as u32).to_le_bytes());
        head.extend_from_slice(e.pattern.as_bytes());
        head.push(e.active as u8);
    }
    head.push(store.excludes.exclude_hidden as u8);
    head.extend_from_slice(&(names.folders.len() as u32).to_le_bytes());
    head.extend_from_slice(&(names.files.len() as u32).to_le_bytes());

    let mut body: Vec<u8> =
        Vec::with_capacity(db.names.len() + 32 * (db.folders.len() + db.files.len()));
    write_entries(
        &mut body,
        db,
        &names.folders,
        true,
        store.flags,
        &folder_pos,
    );
    let folder_block = body.len() as u64;
    write_entries(&mut body, db, &names.files, false, store.flags, &folder_pos);
    let file_block = body.len() as u64 - folder_block;
    let n_sorted = store.sorted.iter().filter(|s| s.is_some()).count() as u32;
    body.extend_from_slice(&n_sorted.to_le_bytes());
    for (id, s) in store.sorted.iter().enumerate() {
        let Some(s) = s else { continue };
        body.extend_from_slice(&(id as u32).to_le_bytes());
        for &f in s.folders.iter() {
            body.extend_from_slice(&folder_pos[f as usize].to_le_bytes());
        }
        for &f in s.files.iter() {
            body.extend_from_slice(&file_pos[f as usize].to_le_bytes());
        }
    }

    head.extend_from_slice(&folder_block.to_le_bytes());
    head.extend_from_slice(&file_block.to_le_bytes());
    let digest = Md5::digest(&head);

    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    let write = || -> std::io::Result<()> {
        let mut f =
            open_locked(&tmp, true).map_err(|_| std::io::Error::other("cannot lock database"))?;
        f.write_all(&head)?;
        f.write_all(&digest)?;
        f.write_all(&body)?;
        f.flush()?;
        Ok(())
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    let _ = std::fs::remove_file(path);
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}
