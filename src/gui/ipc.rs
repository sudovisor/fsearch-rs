//! Single instance: like the GApplication original, a second launch hands its command line to
//! the running instance (search term, preferences, update) and exits.
//!
//! Unix: a socket in a private directory (`$XDG_RUNTIME_DIR`, else `~/.cache/fsearch`, mode 0700),
//! never in shared `/tmp`; a socket file owned by another user is neither used nor removed.
//! Windows: a localhost TCP port recorded in a lock file in the user's local app data, with a
//! per-session token so other users' processes cannot send commands.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::Sender;

pub enum Message {
    Activate,
    Search(String),
    Preferences,
    Update,
}

const MAX_MESSAGE: u64 = 64 * 1024;

fn parse(line: &str) -> Message {
    if let Some(t) = line.strip_prefix("search ") {
        Message::Search(t.to_string())
    } else {
        match line {
            "preferences" => Message::Preferences,
            "update" => Message::Update,
            _ => Message::Activate,
        }
    }
}

fn serve<R: Read>(stream: R, tx: &Sender<Message>, wake: &dyn Fn(), token: Option<&str>) -> bool {
    let mut lines = BufReader::new(stream.take(MAX_MESSAGE)).lines();
    if let Some(t) = token {
        match lines.next() {
            Some(Ok(l)) if l == t => {}
            _ => return true,
        }
    }
    for line in lines.map_while(Result::ok) {
        if tx.send(parse(&line)).is_err() {
            return false;
        }
        wake();
    }
    true
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};

    fn uid() -> u32 {
        std::fs::metadata("/proc/self")
            .map(|m| m.uid())
            .unwrap_or_else(|_| rustix::process::getuid().as_raw())
    }

    fn dir() -> Option<PathBuf> {
        if let Some(d) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)
            && d.is_dir()
        {
            return Some(d);
        }
        let d = std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?
            .join("fsearch");
        std::fs::create_dir_all(&d).ok()?;
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).ok()?;
        Some(d)
    }

    fn socket_path() -> Option<PathBuf> {
        Some(dir()?.join(format!("fsearch-rs-{}.sock", uid())))
    }

    /// The socket file exists and belongs to us (another user's file is never trusted).
    fn ours(p: &std::path::Path) -> bool {
        std::fs::symlink_metadata(p).is_ok_and(|m| m.uid() == uid() && !m.file_type().is_symlink())
    }

    pub fn send(msgs: &[String]) -> bool {
        let Some(p) = socket_path() else { return false };
        if !ours(&p) {
            return false;
        }
        let Ok(mut s) = UnixStream::connect(&p) else {
            return false;
        };
        msgs.iter()
            .all(|m| writeln!(s, "{}", m.replace('\n', " ")).is_ok())
    }

    #[inline]
    fn peer_matches_uid(#[allow(unused_variables)] stream: &UnixStream) -> bool {
        #[cfg(target_os = "linux")]
        {
            if let Ok(cred) = rustix::net::sockopt::socket_peercred(stream) {
                return cred.uid.as_raw() == uid();
            }
            false
        }
        #[cfg(not(target_os = "linux"))]
        {
            true
        }
    }

    pub fn listen(tx: Sender<Message>, wake: Box<dyn Fn() + Send>) {
        let Some(path) = socket_path() else { return };
        if std::fs::symlink_metadata(&path).is_ok() {
            if !ours(&path) {
                return; // not ours: run without single-instance support rather than touch it
            }
            let _ = std::fs::remove_file(&path);
        }
        let Ok(listener) = UnixListener::bind(&path) else {
            return;
        };
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        std::thread::spawn(move || {
            for stream in listener.incoming().map_while(Result::ok) {
                // peers must be the same user
                if !peer_matches_uid(&stream) {
                    continue;
                }
                if !serve(stream, &tx, &*wake, None) {
                    return;
                }
            }
        });
    }

    pub fn cleanup() {
        if let Some(p) = socket_path()
            && ours(&p)
        {
            let _ = std::fs::remove_file(p);
        }
    }
}

#[cfg(not(unix))]
mod imp {
    use super::*;
    use std::net::{Ipv4Addr, TcpListener, TcpStream};

    fn lock_path() -> Option<PathBuf> {
        let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
        let d = base.join("fsearch");
        std::fs::create_dir_all(&d).ok()?;
        Some(d.join("instance"))
    }

    pub fn send(msgs: &[String]) -> bool {
        let Some(p) = lock_path() else { return false };
        let Ok(text) = std::fs::read_to_string(&p) else {
            return false;
        };
        let mut parts = text.split_whitespace();
        let (Some(port), Some(token)) = (
            parts.next().and_then(|x| x.parse::<u16>().ok()),
            parts.next(),
        ) else {
            return false;
        };
        let Ok(mut s) = TcpStream::connect((Ipv4Addr::LOCALHOST, port)) else {
            return false;
        };
        if writeln!(s, "{token}").is_err() {
            return false;
        }
        msgs.iter()
            .all(|m| writeln!(s, "{}", m.replace('\n', " ")).is_ok())
    }

    pub fn listen(tx: Sender<Message>, wake: Box<dyn Fn() + Send>) {
        let Some(p) = lock_path() else { return };
        let Ok(listener) = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)) else {
            return;
        };
        let Ok(addr) = listener.local_addr() else {
            return;
        };
        let token: String = {
            use std::hash::{BuildHasher, Hasher};
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u128(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0),
            );
            h.write_u32(std::process::id());
            format!(
                "{:016x}{:016x}",
                h.finish(),
                std::collections::hash_map::RandomState::new()
                    .build_hasher()
                    .finish()
            )
        };
        if std::fs::write(&p, format!("{} {}", addr.port(), token)).is_err() {
            return;
        }
        std::thread::spawn(move || {
            for stream in listener.incoming().map_while(Result::ok) {
                if !serve(stream, &tx, &*wake, Some(&token)) {
                    return;
                }
            }
        });
    }

    pub fn cleanup() {
        if let Some(p) = lock_path() {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Send to a running instance. Returns false when there is none.
pub fn send(msgs: &[String]) -> bool {
    imp::send(msgs)
}

/// Become the primary instance and forward messages to `tx`.
pub fn listen(tx: Sender<Message>, wake: Box<dyn Fn() + Send>) {
    imp::listen(tx, wake)
}

pub fn cleanup() {
    imp::cleanup()
}
