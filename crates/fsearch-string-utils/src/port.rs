//! The Rust port of `src/fsearch_string_utils.c`: safe Rust; `lib.rs` exports each function under its C name.
//!
//! C has no error channel: return what the C code returns, and never panic (a panic aborts at the C boundary).

#![forbid(unsafe_code)]

use icu_properties::CodePointMapData;
use icu_properties::props::GeneralCategory;

/// glibc `isspace` in a UTF-8 or C locale: bytes >= 0x80 (negative `char`) are never space.
fn c_isspace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// True when the current `LC_CTYPE` is Turkish or Azeri, where GLib lowercases `I` to dotless `ı`.
fn turkic_locale() -> bool {
    let v = ["LC_ALL", "LC_CTYPE", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|s| !s.is_empty()))
        .unwrap_or_default();
    v.starts_with("tr") || v.starts_with("az")
}

/// Port of `bool fsearch_string_is_empty(const char *str)` (`src/fsearch_string_utils.c:26`).
///
/// `str`: the bytes of the C string, without its NUL; `None` for `NULL` (the original asserts).
pub fn fsearch_string_is_empty(str: Option<&[u8]>) -> bool {
    match str {
        Some(s) => s.iter().all(|&b| c_isspace(b)),
        None => true,
    }
}

/// Port of `bool fsearch_string_is_ascii_icase(const char *str)` (`src/fsearch_string_utils.c:41`).
///
/// True when both the lowercase and the uppercase form of the string are pure ASCII, i.e. ASCII
/// case-insensitive comparison gives the same answer as Unicode case folding.
pub fn fsearch_string_is_ascii_icase(str: Option<&[u8]>) -> bool {
    let Some(s) = str else {
        return true;
    };
    if s.is_empty() {
        return true;
    }
    let text = String::from_utf8_lossy(s);
    let turkic = turkic_locale();
    let down_ascii = text.chars().all(|c| {
        if turkic && c == 'I' {
            return false; // I -> ı
        }
        c.to_lowercase().all(|l| l.is_ascii())
    });
    let up_ascii = text.chars().all(|c| {
        if turkic && c == 'i' {
            return false; // i -> İ
        }
        c.to_uppercase().all(|u| u.is_ascii())
    });
    down_ascii && up_ascii
}

/// Port of `bool fsearch_string_utf8_has_upper(const char *str)` (`src/fsearch_string_utils.c:60`).
///
/// `g_unichar_isupper` is general category Lu only (not the wider Uppercase property).
pub fn fsearch_string_utf8_has_upper(str: Option<&[u8]>) -> bool {
    let Some(s) = str else {
        return false;
    };
    let Ok(text) = std::str::from_utf8(s) else {
        return false;
    };
    let gc = CodePointMapData::<GeneralCategory>::new();
    text.chars()
        .any(|c| gc.get(c) == GeneralCategory::UppercaseLetter)
}

/// Port of `bool fsearch_string_has_upper(const char *str)` (`src/fsearch_string_utils.c:77`).
///
/// glibc `isupper` on a (signed) `char` in a UTF-8 locale: ASCII `A`-`Z` only.
pub fn fsearch_string_has_upper(str: Option<&[u8]>) -> bool {
    str.is_some_and(|s| s.iter().any(u8::is_ascii_uppercase))
}

/// Port of `bool fsearch_string_has_wildcards(const char *str)` (`src/fsearch_string_utils.c:118`).
pub fn fsearch_string_has_wildcards(str: Option<&[u8]>) -> bool {
    str.is_some_and(|s| s.iter().any(|&b| b == b'*' || b == b'?'))
}

/// `fsearch_string_get_extension_len`: the extension after the last `.`, or `""` when the name
/// has none, starts with its only dot, or ends with a dot. (Kept in C by the bridge; used by the core.)
pub fn get_extension(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|&b| b == b'.') {
        None | Some(0) => b"",
        Some(i) if i == name.len() - 1 => b"",
        Some(i) => &name[i + 1..],
    }
}

/// `fsearch_string_convert_wildcard_to_regex_expression`: anchored regex for a `*`/`?` pattern.
pub fn convert_wildcard_to_regex(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() + 2);
    out.push(b'^');
    for &c in s {
        match c {
            b'.' | b'^' | b'$' | b'+' | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'\\' | b'|' => {
                out.push(b'\\');
                out.push(c);
            }
            b'*' => out.extend_from_slice(b".*"),
            b'?' => out.push(b'.'),
            _ => out.push(c),
        }
    }
    out.push(b'$');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty() {
        assert!(fsearch_string_is_empty(Some(b"")));
        assert!(fsearch_string_is_empty(Some(b" \t\n")));
        assert!(!fsearch_string_is_empty(Some(b" a ")));
        assert!(!fsearch_string_is_empty(Some("\u{a0}".as_bytes())));
    }

    #[test]
    fn ascii_icase() {
        assert!(fsearch_string_is_ascii_icase(Some(b"abcXYZ")));
        assert!(!fsearch_string_is_ascii_icase(Some("ä".as_bytes())));
        assert!(!fsearch_string_is_ascii_icase(Some("ß".as_bytes())));
        assert!(!fsearch_string_is_ascii_icase(Some("\u{212a}".as_bytes())));
    }

    #[test]
    fn has_upper() {
        assert!(fsearch_string_utf8_has_upper(Some("Ärger".as_bytes())));
        assert!(!fsearch_string_utf8_has_upper(Some("ärger".as_bytes())));
        assert!(!fsearch_string_utf8_has_upper(Some("\u{24b6}".as_bytes()))); // Ⓐ is So, not Lu
        assert!(!fsearch_string_has_upper(Some(
            "Ärger".as_bytes()[..2].as_ref()
        )));
        assert!(fsearch_string_has_upper(Some(b"aB")));
    }

    #[test]
    fn extension_and_wildcards() {
        assert_eq!(get_extension(b"a.tar.gz"), b"gz");
        assert_eq!(get_extension(b".bashrc"), b"");
        assert_eq!(get_extension(b"trailing."), b"");
        assert_eq!(get_extension(b"README"), b"");
        assert_eq!(convert_wildcard_to_regex(b"a*.c?"), b"^a.*\\.c.$");
        assert!(fsearch_string_has_wildcards(Some(b"x?")));
    }
}
