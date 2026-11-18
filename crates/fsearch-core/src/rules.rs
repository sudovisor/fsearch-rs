//! Includes and excludes: port of `fsearch_database_include*.c` and `fsearch_database_exclude*.c`.

use crate::cmp::cmp_paths;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Include {
    pub path: String,
    pub active: bool,
    pub one_file_system: bool,
    pub monitored: bool,
    pub scan_after_launch: bool,
    pub rescan_after: i64,
    pub last_scan_time: i64,
    pub last_scan_duration: u32,
    pub last_error_code: u32,
    pub last_scanned_folder_count: u32,
    pub last_scanned_file_count: u32,
    pub last_scan_reason: u8,
}

impl Include {
    pub fn new(
        path: &str,
        active: bool,
        one_file_system: bool,
        monitored: bool,
        scan_after_launch: bool,
        rescan_after: i64,
    ) -> Include {
        Include {
            path: path.to_string(),
            active,
            one_file_system,
            monitored,
            scan_after_launch,
            rescan_after,
            last_scan_time: 0,
            last_scan_duration: 0,
            last_error_code: 0,
            last_scanned_folder_count: 0,
            last_scanned_file_count: 0,
            last_scan_reason: 0,
        }
    }

    /// `fsearch_database_include_equal`: the user-facing settings, not the scan statistics.
    pub fn same_config(&self, o: &Include) -> bool {
        self.path == o.path
            && self.active == o.active
            && self.one_file_system == o.one_file_system
            && self.monitored == o.monitored
            && self.scan_after_launch == o.scan_after_launch
            && self.rescan_after == o.rescan_after
    }
}

/// `fsearch_database_include_manager_add`: no duplicate paths, kept sorted with `cmp_paths`.
pub fn add_include(list: &mut Vec<Include>, inc: Include) {
    if list.iter().any(|i| i.path == inc.path) {
        return;
    }
    list.push(inc);
    list.sort_by(|a, b| cmp_paths(a.path.as_bytes(), b.path.as_bytes()).cmp(&0));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ExcludeType {
    Fixed = 0,
    Wildcard = 1,
    Regex = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Scope {
    FullPath = 0,
    Basename = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Target {
    Both = 0,
    Files = 1,
    Folders = 2,
}

impl ExcludeType {
    pub fn parse_str(s: &str) -> ExcludeType {
        match s {
            "wildcard" => ExcludeType::Wildcard,
            "regex" => ExcludeType::Regex,
            _ => ExcludeType::Fixed,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            ExcludeType::Fixed => "fixed",
            ExcludeType::Wildcard => "wildcard",
            ExcludeType::Regex => "regex",
        }
    }
    pub fn from_u8(v: u8) -> ExcludeType {
        match v {
            1 => ExcludeType::Wildcard,
            2 => ExcludeType::Regex,
            _ => ExcludeType::Fixed,
        }
    }
}

impl Scope {
    pub fn parse_str(s: &str) -> Scope {
        if s == "basename" {
            Scope::Basename
        } else {
            Scope::FullPath
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::FullPath => "full_path",
            Scope::Basename => "basename",
        }
    }
    pub fn from_u8(v: u8) -> Scope {
        if v == 1 {
            Scope::Basename
        } else {
            Scope::FullPath
        }
    }
}

impl Target {
    pub fn parse_str(s: &str) -> Target {
        match s {
            "files" => Target::Files,
            "folders" => Target::Folders,
            _ => Target::Both,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Target::Both => "both",
            Target::Files => "files",
            Target::Folders => "folders",
        }
    }
    pub fn from_u8(v: u8) -> Target {
        match v {
            1 => Target::Files,
            2 => Target::Folders,
            _ => Target::Both,
        }
    }
}

pub struct Exclude {
    pub pattern: String,
    pub active: bool,
    pub kind: ExcludeType,
    pub scope: Scope,
    pub target: Target,
    regex: Option<pcre2::bytes::Regex>,
}

impl Clone for Exclude {
    fn clone(&self) -> Self {
        Exclude::new(
            &self.pattern,
            self.active,
            self.kind,
            self.scope,
            self.target,
        )
    }
}

impl std::fmt::Debug for Exclude {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Exclude({:?} {:?} {:?} {:?} {})",
            self.pattern, self.kind, self.scope, self.target, self.active
        )
    }
}

impl PartialEq for Exclude {
    fn eq(&self, o: &Exclude) -> bool {
        self.pattern == o.pattern
            && self.active == o.active
            && self.kind == o.kind
            && self.scope == o.scope
            && self.target == o.target
    }
}

impl Exclude {
    pub fn new(
        pattern: &str,
        active: bool,
        kind: ExcludeType,
        scope: Scope,
        target: Target,
    ) -> Exclude {
        // GRegex compiles with UTF-8 enabled by default
        let regex = if kind == ExcludeType::Regex {
            pcre2::bytes::RegexBuilder::new()
                .utf(true)
                .jit_if_available(true)
                .build(pattern)
                .ok()
        } else {
            None
        };
        Exclude {
            pattern: pattern.to_string(),
            active,
            kind,
            scope,
            target,
            regex,
        }
    }

    /// `fsearch_database_exclude_matches`.
    pub fn matches(&self, path: &[u8], basename: &[u8], is_dir: bool) -> bool {
        if self.target == Target::Files && is_dir {
            return false;
        }
        if self.target == Target::Folders && !is_dir {
            return false;
        }
        let input = if self.scope == Scope::FullPath {
            path
        } else {
            basename
        };
        match self.kind {
            ExcludeType::Fixed => self.pattern.as_bytes() == input,
            ExcludeType::Wildcard => pattern_match_simple(self.pattern.as_bytes(), input),
            ExcludeType::Regex => match &self.regex {
                Some(re) => {
                    std::str::from_utf8(input).is_ok() && re.is_match(input).unwrap_or(false)
                }
                None => false,
            },
        }
    }
}

/// `g_pattern_match_simple`: `*` matches any run of characters, `?` exactly one UTF-8 character.
pub fn pattern_match_simple(p: &[u8], s: &[u8]) -> bool {
    fn char_len(b: u8) -> usize {
        match b {
            0x00..=0x7f => 1,
            0xc0..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf7 => 4,
            _ => 1,
        }
    }
    let (mut pi, mut si) = (0usize, 0usize);
    let (mut star_p, mut star_s) = (usize::MAX, 0usize);
    while si < s.len() {
        if pi < p.len() && p[pi] == b'*' {
            star_p = pi;
            star_s = si;
            pi += 1;
            continue;
        }
        if pi < p.len() && p[pi] == b'?' {
            pi += 1;
            si += char_len(s[si]).min(s.len() - si);
            continue;
        }
        if pi < p.len() && p[pi] == s[si] {
            pi += 1;
            si += 1;
            continue;
        }
        if star_p != usize::MAX {
            pi = star_p + 1;
            star_s += char_len(s[star_s]).min(s.len() - star_s);
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExcludeManager {
    pub excludes: Vec<Exclude>,
    pub exclude_hidden: bool,
}

impl ExcludeManager {
    pub fn with_defaults() -> ExcludeManager {
        let d = |p: &str| {
            Exclude::new(
                p,
                true,
                ExcludeType::Fixed,
                Scope::FullPath,
                Target::Folders,
            )
        };
        ExcludeManager {
            excludes: vec![d("/.snapshots"), d("/proc"), d("/sys")],
            exclude_hidden: false,
        }
    }

    /// `fsearch_database_exclude_manager_add`: no exact duplicates.
    pub fn add(&mut self, e: Exclude) {
        if !self.excludes.contains(&e) {
            self.excludes.push(e);
        }
    }

    /// `fsearch_database_exclude_manager_excludes`.
    pub fn excludes(&self, path: &[u8], basename: &[u8], is_dir: bool) -> bool {
        if self.exclude_hidden && basename.first() == Some(&b'.') {
            return true;
        }
        self.excludes
            .iter()
            .any(|e| e.active && e.matches(path, basename, is_dir))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob() {
        assert!(pattern_match_simple(b"*.log", b"a.log"));
        assert!(!pattern_match_simple(b"*.log", b"a.logx"));
        assert!(pattern_match_simple(b"mod?", b"mod1"));
        assert!(!pattern_match_simple(b"mod?", b"mod10"));
        assert!(pattern_match_simple(b"?", "ä".as_bytes()));
    }
}
