//! File type descriptions for the "Type" column and type sorting.
//!
//! The original calls `g_content_type_guess(name, NULL, 0)` and `g_content_type_get_description`,
//! which read the freedesktop shared-mime-info database (xdgmime). This is a port of the
//! filename-only part of xdgmime's glob lookup (`xdgmimeglob.c`) plus the `<comment>` lookup.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

pub const UNKNOWN: &str = "application/octet-stream";

struct Glob {
    weight: i32,
    mime: String,
    pattern: Vec<u8>,
    case_sensitive: bool,
}

#[derive(Default)]
struct Globs {
    literal: Vec<Glob>,
    /// simple `*<suffix>` globs: (suffix, glob)
    simple: Vec<Glob>,
    full: Vec<Glob>,
}

fn data_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match std::env::var_os("XDG_DATA_HOME") {
        Some(d) if !d.is_empty() => v.push(PathBuf::from(d)),
        _ => {
            if let Some(h) = &home {
                v.push(h.join(".local/share"));
            }
        }
    }
    let dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    for d in dirs.split(':').filter(|s| !s.is_empty()) {
        v.push(PathBuf::from(d));
    }
    v
}

fn load_globs() -> Globs {
    let mut g = Globs::default();
    // xdgmime merges the globs of every data directory (user directory first).
    let mut seen_any = false;
    for dir in data_dirs() {
        let Ok(text) = std::fs::read(dir.join("mime/globs2")) else {
            continue;
        };
        seen_any = true;
        for line in text.split(|&b| b == b'\n') {
            if line.is_empty() || line[0] == b'#' {
                continue;
            }
            let parts: Vec<&[u8]> = line.splitn(4, |&b| b == b':').collect();
            if parts.len() < 3 {
                continue;
            }
            let weight = std::str::from_utf8(parts[0])
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(50);
            let mime = String::from_utf8_lossy(parts[1]).into_owned();
            let pattern = parts[2].to_vec();
            let case_sensitive = parts
                .get(3)
                .is_some_and(|f| f.split(|&b| b == b',').any(|x| x == b"cs"));
            // xdgmime ignores a glob it already has for the same type (the first one wins)
            if g.literal
                .iter()
                .chain(&g.simple)
                .chain(&g.full)
                .any(|x| x.pattern == pattern && x.mime == mime)
            {
                continue;
            }
            let glob = Glob {
                weight,
                mime,
                pattern,
                case_sensitive,
            };
            let p = &glob.pattern;
            let special = |b: &u8| matches!(b, b'*' | b'?' | b'[');
            if !p.iter().any(special) {
                g.literal.push(glob);
            } else if p[0] == b'*' && !p[1..].iter().any(special) {
                g.simple.push(glob);
            } else {
                g.full.push(glob);
            }
        }
    }
    if !seen_any {
        return Globs::default();
    }
    g
}

fn globs() -> &'static Globs {
    static G: OnceLock<Globs> = OnceLock::new();
    G.get_or_init(load_globs)
}

/// `fnmatch(pattern, name, 0)` for the subset shared-mime-info uses: `*`, `?`, `[...]`.
fn fnmatch(p: &[u8], s: &[u8]) -> bool {
    let (mut pi, mut si) = (0usize, 0usize);
    let (mut star_p, mut star_s) = (usize::MAX, 0usize);
    while si < s.len() {
        if pi < p.len() {
            match p[pi] {
                b'*' => {
                    star_p = pi;
                    star_s = si;
                    pi += 1;
                    continue;
                }
                b'?' => {
                    pi += 1;
                    si += 1;
                    continue;
                }
                b'[' => {
                    if let Some((matched, next)) = match_class(p, pi, s[si]) {
                        if matched {
                            pi = next;
                            si += 1;
                            continue;
                        }
                    } else if s[si] == b'[' {
                        pi += 1;
                        si += 1;
                        continue;
                    }
                }
                c => {
                    if c == s[si] {
                        pi += 1;
                        si += 1;
                        continue;
                    }
                }
            }
        }
        if star_p != usize::MAX {
            pi = star_p + 1;
            star_s += 1;
            si = star_s;
            continue;
        }
        return false;
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

fn match_class(p: &[u8], start: usize, c: u8) -> Option<(bool, usize)> {
    let mut i = start + 1;
    let negate = i < p.len() && (p[i] == b'!' || p[i] == b'^');
    if negate {
        i += 1;
    }
    let mut matched = false;
    let mut first = true;
    while i < p.len() && (first || p[i] != b']') {
        first = false;
        if i + 2 < p.len() && p[i + 1] == b'-' && p[i + 2] != b']' {
            if p[i] <= c && c <= p[i + 2] {
                matched = true;
            }
            i += 3;
        } else {
            if p[i] == c {
                matched = true;
            }
            i += 1;
        }
    }
    if i >= p.len() {
        return None;
    }
    Some((matched != negate, i + 1))
}

/// Longest-suffix lookup over the simple globs, like `_xdg_glob_hash_node_lookup_file_name`.
fn lookup_simple(g: &Globs, name: &[u8], ignore_case: bool, out: &mut Vec<(String, i32)>) {
    let mut best_len = 0usize;
    let mut found: Vec<(String, i32)> = Vec::new();
    for glob in &g.simple {
        let suffix = &glob.pattern[1..];
        if suffix.is_empty() || suffix.len() > name.len() {
            continue;
        }
        let tail = &name[name.len() - suffix.len()..];
        let ok = if ignore_case {
            !glob.case_sensitive
                && tail
                    .iter()
                    .map(|c| c.to_ascii_lowercase())
                    .eq(suffix.iter().copied())
        } else {
            tail == suffix
        };
        if !ok {
            continue;
        }
        if suffix.len() > best_len {
            best_len = suffix.len();
            found.clear();
        }
        if suffix.len() == best_len {
            found.push((glob.mime.clone(), glob.weight));
        }
    }
    out.extend(found);
}

/// Used where the system has no shared-mime-info (Windows): common extensions with the
/// descriptions shared-mime-info gives them, so the Type column reads the same everywhere.
const BUILTIN: &[(&str, &str, &str)] = &[
    ("txt", "text/plain", "Plain text document"),
    ("pdf", "application/pdf", "PDF document"),
    ("png", "image/png", "PNG image"),
    ("jpg", "image/jpeg", "JPEG image"),
    ("jpeg", "image/jpeg", "JPEG image"),
    ("gif", "image/gif", "GIF image"),
    ("svg", "image/svg+xml", "SVG image"),
    ("webp", "image/webp", "WebP image"),
    ("bmp", "image/bmp", "Windows BMP image"),
    ("ico", "image/vnd.microsoft.icon", "Windows icon"),
    ("tif", "image/tiff", "TIFF image"),
    ("tiff", "image/tiff", "TIFF image"),
    ("mp3", "audio/mpeg", "MP3 audio"),
    ("flac", "audio/flac", "FLAC audio"),
    ("wav", "audio/vnd.wave", "WAV audio"),
    ("ogg", "audio/ogg", "Ogg audio"),
    ("opus", "audio/ogg", "Ogg audio"),
    ("m4a", "audio/mp4", "MPEG-4 audio"),
    ("aac", "audio/aac", "AAC audio"),
    ("mp4", "video/mp4", "MPEG-4 video"),
    ("mkv", "video/x-matroska", "Matroska video"),
    ("webm", "video/webm", "WebM video"),
    ("avi", "video/vnd.avi", "AVI video"),
    ("mov", "video/quicktime", "QuickTime video"),
    ("wmv", "video/x-ms-wmv", "Windows Media video"),
    ("flv", "video/x-flv", "Flash video"),
    ("zip", "application/zip", "Zip archive"),
    ("7z", "application/x-7z-compressed", "7-zip archive"),
    ("rar", "application/vnd.rar", "RAR archive"),
    ("tar", "application/x-tar", "Tar archive"),
    ("gz", "application/gzip", "Gzip archive"),
    (
        "tgz",
        "application/x-compressed-tar",
        "Tar archive (gzip-compressed)",
    ),
    ("bz2", "application/x-bzip2", "Bzip2 archive"),
    ("xz", "application/x-xz", "XZ archive"),
    ("zst", "application/zstd", "Zstandard archive"),
    ("iso", "application/vnd.efi.iso", "Raw CD image"),
    ("exe", "application/x-msdownload", "Windows or DOS program"),
    ("msi", "application/x-msi", "Windows Installer package"),
    ("dll", "application/x-msdownload", "Windows or DOS program"),
    (
        "sys",
        "application/vnd.microsoft.portable-executable",
        "Windows or EFI program",
    ),
    ("bat", "application/x-bat", "DOS/Windows batch file"),
    ("cmd", "application/x-cmd", "Windows command script"),
    ("ps1", "application/x-powershell", "PowerShell script"),
    ("sh", "application/x-shellscript", "Shell script"),
    ("py", "text/x-python", "Python script"),
    ("rs", "text/rust", "Rust source code"),
    ("c", "text/x-csrc", "C source code"),
    ("h", "text/x-chdr", "C header"),
    ("cpp", "text/x-c++src", "C++ source code"),
    ("hpp", "text/x-c++hdr", "C++ header"),
    ("cs", "text/x-csharp", "C# source code"),
    ("java", "text/x-java", "Java source code"),
    ("js", "text/javascript", "JavaScript program"),
    ("ts", "text/vnd.trolltech.linguist", "Message catalog"),
    ("json", "application/json", "JSON document"),
    ("xml", "application/xml", "XML document"),
    ("html", "text/html", "HTML document"),
    ("htm", "text/html", "HTML document"),
    ("css", "text/css", "CSS stylesheet"),
    ("md", "text/markdown", "Markdown document"),
    ("csv", "text/csv", "CSV document"),
    ("ini", "application/x-ini", "INI file"),
    ("toml", "application/toml", "TOML document"),
    ("yaml", "application/yaml", "YAML document"),
    ("yml", "application/yaml", "YAML document"),
    ("log", "text/x-log", "Application log"),
    ("doc", "application/msword", "Word document"),
    (
        "docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "Word 2007 document",
    ),
    ("xls", "application/vnd.ms-excel", "Excel spreadsheet"),
    (
        "xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "Excel 2007 spreadsheet",
    ),
    (
        "ppt",
        "application/vnd.ms-powerpoint",
        "PowerPoint presentation",
    ),
    (
        "pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "PowerPoint 2007 presentation",
    ),
    (
        "odt",
        "application/vnd.oasis.opendocument.text",
        "ODT document",
    ),
    (
        "ods",
        "application/vnd.oasis.opendocument.spreadsheet",
        "ODS spreadsheet",
    ),
    (
        "odp",
        "application/vnd.oasis.opendocument.presentation",
        "ODP presentation",
    ),
    ("rtf", "application/rtf", "RTF document"),
    ("epub", "application/epub+zip", "Electronic book document"),
    ("ttf", "font/ttf", "TrueType font"),
    (
        "otf",
        "application/vnd.oasis.opendocument.formula-template",
        "ODF template",
    ),
    ("woff", "font/woff", "WOFF font"),
    ("woff2", "font/woff2", "WOFF2 font"),
    ("db", "application/x-db", "DB file"),
    ("sqlite", "application/x-sqlite", "SQLITE file"),
    ("lnk", "application/x-ms-shortcut", "Windows shortcut"),
    ("url", "application/x-mswinurl", "Internet shortcut"),
    ("desktop", "application/x-desktop", "Desktop entry"),
    (
        "deb",
        "application/vnd.debian.binary-package",
        "Debian package",
    ),
    ("rpm", "application/x-rpm", "RPM package"),
    (
        "apk",
        "application/vnd.android.package-archive",
        "Android package",
    ),
    ("jar", "application/java-archive", "Java archive"),
    ("class", "application/x-java", "Java class"),
    ("go", "text/x-go", "Go source code"),
    ("rb", "application/x-ruby", "Ruby script"),
    ("php", "application/x-php", "PHP script"),
    ("pl", "application/x-perl", "Perl script"),
    ("lua", "text/x-lua", "Lua script"),
    ("sql", "application/sql", "SQL code"),
    ("diff", "text/x-patch", "Differences between files"),
    ("patch", "text/x-patch", "Differences between files"),
];

fn builtin_guess(name: &[u8]) -> Option<&'static (&'static str, &'static str, &'static str)> {
    let ext = name
        .rsplit(|&b| b == b'.')
        .next()
        .filter(|_| name.contains(&b'.'))?;
    let lower = ext.to_ascii_lowercase();
    BUILTIN
        .iter()
        .find(|(e, _, _)| e.as_bytes() == lower.as_slice())
}

/// `g_content_type_guess(name, NULL, 0)`.
pub fn guess(name: &[u8]) -> String {
    let g = globs();
    if g.literal.is_empty() && g.simple.is_empty() && g.full.is_empty() {
        return builtin_guess(name)
            .map(|b| b.1.to_string())
            .unwrap_or_else(|| UNKNOWN.to_string());
    }
    for l in &g.literal {
        if l.pattern == name {
            return l.mime.clone();
        }
    }
    for l in &g.literal {
        if !l.case_sensitive && l.pattern.eq_ignore_ascii_case(name) {
            return l.mime.clone();
        }
    }
    let mut mimes: Vec<(String, i32)> = Vec::new();
    lookup_simple(g, name, false, &mut mimes);
    if mimes.len() < 2 {
        lookup_simple(g, name, true, &mut mimes);
    }
    if mimes.len() < 2 {
        let lower = name.to_ascii_lowercase();
        for f in &g.full {
            if fnmatch(&f.pattern, if f.case_sensitive { name } else { &lower }) {
                mimes.push((f.mime.clone(), f.weight));
            }
        }
    }
    let mut dedup: Vec<(String, i32)> = Vec::new();
    for m in mimes {
        if let Some(existing) = dedup.iter_mut().find(|(t, _)| *t == m.0) {
            existing.1 = existing.1.max(m.1);
        } else {
            dedup.push(m);
        }
    }
    dedup.sort_by_key(|m| std::cmp::Reverse(m.1));
    dedup
        .into_iter()
        .next()
        .map(|(m, _)| m)
        .unwrap_or_else(|| UNKNOWN.to_string())
}

fn language_names() -> Vec<String> {
    let raw = ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "C".into());
    let mut out = Vec::new();
    for lang in raw.split(':') {
        let base = lang.split(['.', '@']).next().unwrap_or("");
        if base.is_empty() || base == "C" || base == "POSIX" {
            continue;
        }
        out.push(base.replace('_', "-"));
        if let Some((l, _)) = base.split_once('_') {
            out.push(l.to_string());
        }
    }
    out
}

fn read_comment(mime: &str) -> Option<String> {
    let langs = language_names();
    for dir in data_dirs() {
        let Ok(xml) = std::fs::read_to_string(dir.join("mime").join(format!("{mime}.xml"))) else {
            continue;
        };
        let mut default: Option<String> = None;
        let mut best: Option<(usize, String)> = None;
        let mut rest = xml.as_str();
        while let Some(i) = rest.find("<comment") {
            rest = &rest[i + 8..];
            let Some(close) = rest.find('>') else { break };
            let attrs = &rest[..close];
            let Some(end) = rest.find("</comment>") else {
                break;
            };
            let text = unescape(&rest[close + 1..end]);
            rest = &rest[end..];
            match attrs.find("xml:lang=\"") {
                None => {
                    if default.is_none() {
                        default = Some(text);
                    }
                }
                Some(a) => {
                    let v = &attrs[a + 10..];
                    let lang = &v[..v.find('"').unwrap_or(v.len())];
                    if let Some(pos) = langs.iter().position(|l| l == lang)
                        && best.as_ref().is_none_or(|(p, _)| pos < *p)
                    {
                        best = Some((pos, text));
                    }
                }
            }
        }
        if let Some((_, t)) = best {
            return Some(t);
        }
        if default.is_some() {
            return default;
        }
    }
    None
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn aliases() -> &'static HashMap<String, String> {
    static A: OnceLock<HashMap<String, String>> = OnceLock::new();
    A.get_or_init(|| {
        let mut m = HashMap::new();
        for dir in data_dirs() {
            let Ok(text) = std::fs::read_to_string(dir.join("mime/aliases")) else {
                continue;
            };
            for line in text.lines() {
                if let Some((alias, canonical)) = line.split_once(' ') {
                    m.entry(alias.to_string())
                        .or_insert_with(|| canonical.to_string());
                }
            }
        }
        m
    })
}

/// `g_content_type_get_description` (which unaliases the type first).
pub fn description(mime: &str) -> String {
    let mime = aliases().get(mime).map(String::as_str).unwrap_or(mime);
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Ok(c) = cache.lock()
        && let Some(d) = c.get(mime)
    {
        return d.clone();
    }
    let d = read_comment(mime)
        .or_else(|| {
            BUILTIN
                .iter()
                .find(|b| b.1 == mime)
                .map(|b| b.2.to_string())
        })
        .unwrap_or_else(|| {
            if mime == UNKNOWN {
                "Unknown".to_string()
            } else {
                format!("{mime} type")
            }
        });
    if let Ok(mut c) = cache.lock() {
        c.insert(mime.to_string(), d.clone());
    }
    d
}

/// `fsearch_file_utils_get_file_type_non_localized`.
pub fn file_type(name: &[u8], is_dir: bool) -> String {
    if is_dir {
        return "Folder".into();
    }
    description(&guess(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globbing() {
        assert!(fnmatch(b"*.so.[0-9]*", b"libx.so.1"));
        assert!(!fnmatch(b"*.so.[0-9]*", b"libx.so.x"));
        assert!(fnmatch(b"readme*", b"readme.md"));
    }
}
