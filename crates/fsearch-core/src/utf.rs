//! Port of `FsearchUtfBuilder` (`src/fsearch_utf.c`): full Unicode case folding followed by NFD,
//! so that case-insensitive matching treats `ß`/`SS`, `é`/`e\u{301}` and friends the way ICU does.

use icu_casemap::CaseMapper;
use icu_normalizer::DecomposingNormalizer;
use std::sync::OnceLock;

/// Turkish/Azeri locale: `I`/`i` fold to dotless/dotted forms.
pub fn turkic() -> bool {
    static T: OnceLock<bool> = OnceLock::new();
    *T.get_or_init(|| {
        let v = ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .find_map(|k| std::env::var(k).ok().filter(|s| !s.is_empty()))
            .unwrap_or_default();
        v.starts_with("tr") || v.starts_with("az")
    })
}

/// Case-fold and NFD-normalize `s`. Returns `None` when the original's fixed buffer
/// (`4 * capacity_hint` UTF-16 units) would overflow, which makes the match fail there too.
pub fn fold_normalize(s: &[u8], capacity_units: usize) -> Option<String> {
    let text = String::from_utf8_lossy(s);
    let cm = CaseMapper::new();
    let folded = if turkic() {
        cm.fold_turkic_string(&text)
    } else {
        cm.fold_string(&text)
    };
    if folded.len() > capacity_units {
        return None; // the folded UTF-8 buffer holds `capacity_units` bytes
    }
    let out = DecomposingNormalizer::new_nfd()
        .normalize(&folded)
        .into_owned();
    if out.encode_utf16().count() > capacity_units {
        return None;
    }
    Some(out)
}

/// Needle capacity: `4 * strlen(needle)` UTF-16 units.
pub fn fold_needle(s: &[u8]) -> Option<String> {
    fold_normalize(s, 4 * s.len())
}

/// The original's haystack buffer: `4 * PATH_MAX`.
pub const HAYSTACK_CAPACITY: usize = 4 * 4096;

/// Haystack capacity: `4 * PATH_MAX` UTF-16 units.
pub fn fold_haystack(s: &[u8]) -> Option<String> {
    fold_normalize(s, 4 * 4096)
}

/// [`fold_normalize`] into reusable buffers (`tmp` for the folded text, `out` for the result);
/// returns false where the original's fixed buffers would overflow.
pub fn fold_normalize_into(
    s: &[u8],
    capacity_units: usize,
    tmp: &mut String,
    out: &mut String,
) -> bool {
    use writeable::Writeable;
    let lossy;
    let text = match std::str::from_utf8(s) {
        Ok(t) => t,
        Err(_) => {
            lossy = String::from_utf8_lossy(s).into_owned();
            &lossy
        }
    };
    tmp.clear();
    let cm = CaseMapper::new();
    let written = if turkic() {
        cm.fold_turkic(text).write_to(tmp)
    } else {
        cm.fold(text).write_to(tmp)
    };
    if written.is_err() || tmp.len() > capacity_units {
        return false;
    }
    out.clear();
    let mut units = 0usize;
    for c in DecomposingNormalizer::new_nfd().normalize_iter(tmp.chars()) {
        units += c.len_utf16();
        out.push(c);
    }
    units <= capacity_units
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds() {
        assert_eq!(fold_needle("STRAẞE".as_bytes()).as_deref(), Some("strasse"));
        assert_eq!(
            fold_needle("Café".as_bytes()),
            fold_needle("cafe\u{301}".as_bytes())
        );
    }
}
