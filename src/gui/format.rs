//! Display formats of the result list (`fsearch_result_view.c`, `fsearch_file_utils.c`).

/// `g_format_size_full(size, DEFAULT | IEC_UNITS)`.
pub fn size(bytes: i64, base2: bool) -> String {
    let b = bytes.max(0) as u64;
    let (base, units): (f64, [&str; 6]) = if base2 {
        (1024.0, ["KiB", "MiB", "GiB", "TiB", "PiB", "EiB"])
    } else {
        (1000.0, ["kB", "MB", "GB", "TB", "PB", "EB"])
    };
    if (b as f64) < base {
        return if b == 1 {
            "1 byte".into()
        } else {
            format!("{b} bytes")
        };
    }
    let mut v = b as f64 / base;
    let mut i = 0;
    while v >= base && i < units.len() - 1 {
        v /= base;
        i += 1;
    }
    format!("{v:.1} {}", units[i])
}

/// `strftime("%Y-%m-%d %H:%M", localtime(mtime))`.
pub fn date(mtime: i64) -> String {
    use chrono::{Local, TimeZone};
    match Local.timestamp_opt(mtime, 0) {
        chrono::LocalResult::Single(t) | chrono::LocalResult::Ambiguous(t, _) => {
            t.format("%Y-%m-%d %H:%M").to_string()
        }
        chrono::LocalResult::None => String::new(),
    }
}

/// `%'d`: thousands separators.
pub fn count(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn sizes() {
        assert_eq!(super::size(999, false), "999 bytes");
        assert_eq!(super::size(1500, false), "1.5 kB");
        assert_eq!(super::size(1536, true), "1.5 KiB");
        assert_eq!(super::count(1234567), "1,234,567");
    }
}
