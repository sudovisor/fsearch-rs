//! Port of `fsearch_size_parse` (`src/fsearch_size_utils.c`) and the `strtoll` it relies on.

/// glibc `strtoll(str, &end, 10)`: returns the value (saturated on overflow) and the index where
/// parsing stopped; the index is 0 when no digits were read.
pub fn strtoll(s: &[u8]) -> (i64, usize) {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let digits_start = i;
    let mut acc: i128 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        acc = (acc * 10 + (s[i] - b'0') as i128).min(i64::MAX as i128 + 1);
        i += 1;
    }
    if i == digits_start {
        return (0, 0);
    }
    let v = if neg { -acc } else { acc };
    (v.clamp(i64::MIN as i128, i64::MAX as i128) as i64, i)
}

/// `fsearch_size_parse`: a byte count with an optional `k`/`m`/`g`/`t` (+ optional `b`) suffix.
/// Returns `(size, size_end)`; `size_end` widens the value to everything that rounds to it.
pub fn parse(s: &[u8]) -> Option<(i64, i64)> {
    let (mut size, end) = strtoll(s);
    if end == 0 {
        return None;
    }
    let mut rest = &s[end..];
    let mut plus: i64 = 0;
    if let Some(&c) = rest.first() {
        let (mul, p): (i64, i64) = match c {
            b'k' | b'K' => (1000, 1000 - 50 - 1),
            b'm' | b'M' => (1_000_000, 1000 * (1000 - 50) - 1),
            b'g' | b'G' => (1_000_000_000, 1_000_000 * (1000 - 50) - 1),
            b't' | b'T' => (1_000_000_000_000, 1_000_000_000 * (1000 - 50) - 1),
            _ => (0, 0),
        };
        if mul != 0 {
            size = size.wrapping_mul(mul);
            plus = p;
            rest = &rest[1..];
            if let Some(&b) = rest.first()
                && (b == b'b' || b == b'B')
            {
                rest = &rest[1..];
            }
        }
    }
    if !rest.is_empty() {
        return None;
    }
    Some((size, size.wrapping_add(plus)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(parse(b"10"), Some((10, 10)));
        assert_eq!(parse(b"1kb"), Some((1000, 1949)));
        assert_eq!(parse(b"2M"), Some((2_000_000, 2_949_999)));
        assert_eq!(parse(b"abc"), None);
        assert_eq!(parse(b"1x"), None);
        assert_eq!(parse(b"1kx"), None);
    }
}
