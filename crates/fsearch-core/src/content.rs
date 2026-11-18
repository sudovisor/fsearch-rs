//! `contenttype:` support: the GIO content type of a file on disk (`db_entry_append_content_type`).
//!
//! GIO guesses from the file name and, when that is not conclusive, sniffs the first bytes. This
//! port uses the same shared-mime-info globs and a small sniff (empty file, text vs binary).

use std::io::Read;

pub fn content_type_for_path(path: &[u8]) -> String {
    #[cfg(unix)]
    let p = {
        use std::os::unix::ffi::OsStrExt;
        std::path::PathBuf::from(std::ffi::OsStr::from_bytes(path))
    };
    #[cfg(not(unix))]
    let p = std::path::PathBuf::from(String::from_utf8_lossy(path).into_owned());
    let p = p.as_path();
    let Ok(meta) = std::fs::metadata(p) else {
        return "unknown".into();
    };
    if meta.is_dir() {
        return "inode/directory".into();
    }
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let guess = crate::mime::guess(name.as_bytes());
    if guess != crate::mime::UNKNOWN {
        return guess;
    }
    if !meta.is_file() {
        return crate::mime::UNKNOWN.into();
    }
    if meta.len() == 0 {
        return "application/x-zerosize".into();
    }
    let mut buf = vec![0u8; 4096];
    let n = std::fs::File::open(p)
        .and_then(|mut f| f.read(&mut buf))
        .unwrap_or(0);
    let data = &buf[..n];
    if !data.contains(&0) && std::str::from_utf8(data).is_ok() {
        return "text/plain".into();
    }
    crate::mime::UNKNOWN.into()
}
