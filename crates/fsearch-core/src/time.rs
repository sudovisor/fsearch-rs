//! Port of `fsearch_date_time_parse_interval` (`src/fsearch_time_utils.c`).
//!
//! The original relies on libc `mktime`, `localtime` and `strptime` (local time zone, DST
//! normalization, glibc's parsing rules). To keep exactly those semantics this module calls the
//! same libc functions; all `unsafe` code of the crate's date handling is in `ctime`.

use chrono::{Datelike, NaiveDate};

#[cfg(unix)]
mod ctime {
    //! Minimal safe wrappers around libc time functions.
    use std::ffi::CString;

    pub type Tm = libc::tm;

    pub fn zero_tm() -> Tm {
        // SAFETY: `libc::tm` is a plain C struct of integers (and a nullable `tm_zone` pointer);
        // all-zero is a valid value.
        unsafe { std::mem::zeroed() }
    }

    pub fn now() -> i64 {
        // SAFETY: `time(NULL)` has no preconditions.
        unsafe { libc::time(std::ptr::null_mut()) as i64 }
    }

    pub fn localtime(t: i64) -> Tm {
        let mut out = zero_tm();
        let tt = t as libc::time_t;
        // SAFETY: both pointers reference valid, live stack values for the duration of the call.
        unsafe { libc::localtime_r(&tt, &mut out) };
        out
    }

    /// `mktime`, which also normalizes `tm` in place.
    pub fn mktime(tm: &mut Tm) -> i64 {
        // SAFETY: `tm` is a valid, exclusively borrowed `struct tm`.
        unsafe { libc::mktime(tm) as i64 }
    }

    /// `strptime(s, fmt, tm)`: `Some(rest)` with the unparsed suffix on success.
    pub fn strptime<'a>(s: &'a [u8], fmt: &str, tm: &mut Tm) -> Option<&'a [u8]> {
        let cs = CString::new(s).ok()?;
        let cf = CString::new(fmt).ok()?;
        // SAFETY: both strings are NUL-terminated and outlive the call; `tm` is valid and
        // exclusively borrowed. The returned pointer points into `cs`'s buffer.
        let end = unsafe { libc::strptime(cs.as_ptr(), cf.as_ptr(), tm) };
        if end.is_null() {
            return None;
        }
        // SAFETY: `end` points into `cs` (between its start and its NUL terminator).
        let consumed = unsafe { end.offset_from(cs.as_ptr()) } as usize;
        Some(&s[consumed.min(s.len())..])
    }
}

/// Windows has no `strptime`/`localtime_r`: the same operations through chrono's local time
/// zone, normalizing out-of-range fields the way `mktime` does.
#[cfg(not(unix))]
use chrono_ctime as ctime;

#[cfg(any(not(unix), test))]
// on unix only the comparison test against libc uses it
#[cfg_attr(unix, allow(dead_code))]
mod chrono_ctime {
    use chrono::{Datelike, Local, LocalResult, NaiveDate, TimeZone, Timelike};

    #[derive(Clone, Copy, Default, Debug)]
    pub struct Tm {
        pub tm_sec: i32,
        pub tm_min: i32,
        pub tm_hour: i32,
        pub tm_mday: i32,
        pub tm_mon: i32,
        pub tm_year: i32,
        pub tm_wday: i32,
        pub tm_yday: i32,
        pub tm_isdst: i32,
    }

    pub fn zero_tm() -> Tm {
        Tm::default()
    }

    pub fn now() -> i64 {
        chrono::Utc::now().timestamp()
    }

    pub fn localtime(t: i64) -> Tm {
        match Local.timestamp_opt(t, 0) {
            LocalResult::Single(d) | LocalResult::Ambiguous(d, _) => Tm {
                tm_sec: d.second() as i32,
                tm_min: d.minute() as i32,
                tm_hour: d.hour() as i32,
                tm_mday: d.day() as i32,
                tm_mon: d.month0() as i32,
                tm_year: d.year() - 1900,
                tm_wday: d.weekday().num_days_from_sunday() as i32,
                tm_yday: d.ordinal0() as i32,
                tm_isdst: 0,
            },
            LocalResult::None => Tm::default(),
        }
    }

    pub fn mktime(tm: &mut Tm) -> i64 {
        let months = (tm.tm_year as i64 + 1900) * 12 + tm.tm_mon as i64;
        let (y, m0) = (months.div_euclid(12) as i32, months.rem_euclid(12) as u32);
        let Some(first) = NaiveDate::from_ymd_opt(y, m0 + 1, 1) else {
            return -1;
        };
        let secs = (tm.tm_mday as i64 - 1) * 86_400
            + tm.tm_hour as i64 * 3600
            + tm.tm_min as i64 * 60
            + tm.tm_sec as i64;
        let Some(ndt) = first
            .and_hms_opt(0, 0, 0)
            .and_then(|d| d.checked_add_signed(chrono::Duration::seconds(secs)))
        else {
            return -1;
        };
        let t = match Local.from_local_datetime(&ndt) {
            LocalResult::Single(d) | LocalResult::Ambiguous(d, _) => d.timestamp(),
            // a skipped hour: like mktime, move forward by the gap
            LocalResult::None => {
                match Local.from_local_datetime(&(ndt + chrono::Duration::hours(1))) {
                    LocalResult::Single(d) | LocalResult::Ambiguous(d, _) => d.timestamp(),
                    LocalResult::None => return -1,
                }
            }
        };
        *tm = localtime(t);
        t
    }

    fn number(s: &[u8], i: &mut usize, max_digits: usize) -> Option<i32> {
        while *i < s.len() && s[*i] == b' ' {
            *i += 1;
        }
        let start = *i;
        let mut v: i32 = 0;
        while *i < s.len() && *i - start < max_digits && s[*i].is_ascii_digit() {
            v = v * 10 + (s[*i] - b'0') as i32;
            *i += 1;
        }
        (*i > start).then_some(v)
    }

    /// The `strptime` subset the date parser uses (`%Y %y %m %d %H %M %S`), glibc rules.
    pub fn strptime<'a>(s: &'a [u8], fmt: &str, tm: &mut Tm) -> Option<&'a [u8]> {
        let f = fmt.as_bytes();
        let (mut i, mut k) = (0usize, 0usize);
        while k < f.len() {
            if f[k] == b'%' && k + 1 < f.len() {
                let (v, ok) = match f[k + 1] {
                    b'Y' => (number(s, &mut i, 4)?, true),
                    b'y' => {
                        let v = number(s, &mut i, 2)?;
                        (if v < 69 { v + 100 } else { v }, true)
                    }
                    b'm' => (number(s, &mut i, 2)?, true),
                    b'd' => (number(s, &mut i, 2)?, true),
                    b'H' => (number(s, &mut i, 2)?, true),
                    b'M' => (number(s, &mut i, 2)?, true),
                    b'S' => (number(s, &mut i, 2)?, true),
                    _ => return None,
                };
                if !ok {
                    return None;
                }
                match f[k + 1] {
                    b'Y' => tm.tm_year = v - 1900,
                    b'y' => tm.tm_year = v,
                    b'm' if (1..=12).contains(&v) => tm.tm_mon = v - 1,
                    b'd' if (1..=31).contains(&v) => tm.tm_mday = v,
                    b'H' if v <= 23 => tm.tm_hour = v,
                    b'M' if v <= 59 => tm.tm_min = v,
                    b'S' if v <= 61 => tm.tm_sec = v,
                    _ => return None,
                }
                k += 2;
            } else if f[k] == b' ' {
                while i < s.len() && s[i].is_ascii_whitespace() {
                    i += 1;
                }
                k += 1;
            } else {
                if i >= s.len() || s[i] != f[k] {
                    return None;
                }
                i += 1;
                k += 1;
            }
        }
        Some(&s[i..])
    }
}

use ctime::Tm;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Second,
    Minute,
    Hour,
    Week,
    Day,
    Month,
    Year,
}

const DATE_CONSTANTS: &[(&str, i32, Kind)] = &[
    ("today", 0, Kind::Day),
    ("yesterday", 1, Kind::Day),
    ("monday", 1, Kind::Week),
    ("mon", 1, Kind::Week),
    ("tuesday", 2, Kind::Week),
    ("tue", 2, Kind::Week),
    ("wednesday", 3, Kind::Week),
    ("wed", 3, Kind::Week),
    ("thursday", 4, Kind::Week),
    ("thu", 4, Kind::Week),
    ("friday", 5, Kind::Week),
    ("fri", 5, Kind::Week),
    ("saturday", 6, Kind::Week),
    ("sat", 6, Kind::Week),
    ("sunday", 7, Kind::Week),
    ("sun", 7, Kind::Week),
    ("january", 1, Kind::Month),
    ("jan", 1, Kind::Month),
    ("february", 2, Kind::Month),
    ("feb", 2, Kind::Month),
    ("march", 3, Kind::Month),
    ("mar", 3, Kind::Month),
    ("april", 4, Kind::Month),
    ("apr", 4, Kind::Month),
    ("may", 5, Kind::Month),
    ("june", 6, Kind::Month),
    ("jun", 6, Kind::Month),
    ("july", 7, Kind::Month),
    ("jul", 7, Kind::Month),
    ("august", 8, Kind::Month),
    ("aug", 8, Kind::Month),
    ("september", 9, Kind::Month),
    ("sep", 9, Kind::Month),
    ("october", 10, Kind::Month),
    ("oct", 10, Kind::Month),
    ("november", 11, Kind::Month),
    ("nov", 11, Kind::Month),
    ("december", 12, Kind::Month),
    ("dec", 12, Kind::Month),
];

const NUM_CONSTANTS: &[(&str, i64)] = &[
    ("one", 1),
    ("two", 2),
    ("three", 3),
    ("four", 4),
    ("five", 5),
    ("six", 6),
    ("seven", 7),
    ("eight", 8),
    ("nine", 9),
    ("ten", 10),
    ("dozen", 12),
    ("hundred", 100),
];

const PAST_FINAL: i32 = 0;
const PAST: i32 = 1;
const NOW: i32 = 2;

const PREFIXES: &[(&str, i32)] = &[
    ("last", PAST_FINAL),
    ("inthelast", PAST_FINAL),
    ("past", PAST),
    ("previous", PAST_FINAL),
    ("prev", PAST_FINAL),
    ("this", NOW),
];

const SINGULAR: &[(&str, Kind)] = &[
    ("year", Kind::Year),
    ("month", Kind::Month),
    ("week", Kind::Week),
    ("day", Kind::Day),
    ("hour", Kind::Hour),
    ("min", Kind::Minute),
    ("minute", Kind::Minute),
    ("sec", Kind::Second),
    ("second", Kind::Second),
];

const PLURAL: &[(&str, Kind)] = &[
    ("years", Kind::Year),
    ("months", Kind::Month),
    ("weeks", Kind::Week),
    ("days", Kind::Day),
    ("hours", Kind::Hour),
    ("min", Kind::Minute),
    ("minutes", Kind::Minute),
    ("sec", Kind::Second),
    ("seconds", Kind::Second),
];

fn unix_from_tm(tm: &mut Tm) -> i64 {
    ctime::mktime(tm).max(0)
}

/// `g_date_to_struct_tm`.
fn date_to_tm(d: NaiveDate) -> Tm {
    let mut tm = ctime::zero_tm();
    tm.tm_year = d.year() - 1900;
    tm.tm_mon = d.month0() as i32;
    tm.tm_mday = d.day() as i32;
    tm.tm_wday = d.weekday().num_days_from_sunday() as i32;
    tm.tm_yday = d.ordinal0() as i32;
    tm.tm_isdst = -1;
    tm
}

fn add_months(d: NaiveDate, months: i32) -> NaiveDate {
    // g_date_add_months / g_date_subtract_months clamp the day to the target month's length.
    let total = d.year() * 12 + d.month0() as i32 + months;
    let (y, m0) = (total.div_euclid(12), total.rem_euclid(12) as u32);
    let mut day = d.day();
    loop {
        if let Some(nd) = NaiveDate::from_ymd_opt(y, m0 + 1, day) {
            return nd;
        }
        day -= 1;
    }
}

fn parse_explicit(s: &[u8]) -> Option<(Tm, Tm)> {
    let &(_, val, kind) = DATE_CONSTANTS.iter().find(|(f, _, _)| f.as_bytes() == s)?;
    let now = ctime::localtime(ctime::now());
    let mut date = NaiveDate::from_ymd_opt(
        now.tm_year + 1900,
        (now.tm_mon + 1) as u32,
        now.tm_mday as u32,
    )?;
    let (start, end);
    match kind {
        Kind::Week => {
            let wd = date.weekday().number_from_monday() as i32;
            let mut diff = wd - val;
            if diff < 0 {
                diff += 7;
            }
            date = date - chrono::Days::new(diff as u64);
            start = date_to_tm(date);
            end = date_to_tm(date + chrono::Days::new(1));
        }
        Kind::Day => {
            date = date - chrono::Days::new(val as u64);
            start = date_to_tm(date);
            end = date_to_tm(date + chrono::Days::new(1));
        }
        Kind::Month => {
            date = date - chrono::Days::new((date.day() - 1) as u64);
            let mut diff = date.month() as i32 - val;
            if diff < 0 {
                diff += 12;
            }
            date = add_months(date, -diff);
            start = date_to_tm(date);
            end = date_to_tm(add_months(date, 1));
        }
        _ => {
            start = ctime::zero_tm();
            end = ctime::zero_tm();
        }
    }
    Some((start, end))
}

fn subtract_from_tm(tm: Tm, kind: Kind, num: i32) -> Tm {
    let mut r = tm;
    // the original normalizes its copy of the input too; glibc's mktime caches the last offset,
    // which decides ambiguous (DST fall-back) wall times, so the call sequence is kept
    let mut input = tm;
    ctime::mktime(&mut input);
    match kind {
        Kind::Year => r.tm_year -= num,
        Kind::Month => r.tm_mon -= num,
        Kind::Week => r.tm_mday -= 7 * num,
        Kind::Day => r.tm_mday -= num,
        Kind::Hour => r.tm_hour -= num,
        Kind::Minute => r.tm_min -= num,
        Kind::Second => {}
    }
    ctime::mktime(&mut r);
    r
}

fn round_down(tm: Tm, kind: Kind) -> Tm {
    let mut r = ctime::zero_tm();
    r.tm_mday = 1;
    r.tm_isdst = tm.tm_isdst;
    let mut input = tm;
    ctime::mktime(&mut input);
    let tm = input;
    match kind {
        Kind::Minute => {
            r.tm_min = tm.tm_min;
            r.tm_hour = tm.tm_hour;
            r.tm_mday = tm.tm_mday;
            r.tm_mon = tm.tm_mon;
            r.tm_year = tm.tm_year;
        }
        Kind::Hour => {
            r.tm_hour = tm.tm_hour;
            r.tm_mday = tm.tm_mday;
            r.tm_mon = tm.tm_mon;
            r.tm_year = tm.tm_year;
        }
        Kind::Day => {
            r.tm_mday = tm.tm_mday;
            r.tm_mon = tm.tm_mon;
            r.tm_year = tm.tm_year;
        }
        Kind::Month => {
            r.tm_mon = tm.tm_mon;
            r.tm_year = tm.tm_year;
        }
        Kind::Year => r.tm_year = tm.tm_year,
        Kind::Week => {
            r.tm_mday = tm.tm_mday - if tm.tm_wday == 0 { 6 } else { tm.tm_wday - 1 };
            r.tm_mon = tm.tm_mon;
            r.tm_year = tm.tm_year;
        }
        Kind::Second => {}
    }
    ctime::mktime(&mut r);
    r
}

fn starts_with(s: &[u8], p: &str) -> bool {
    s.starts_with(p.as_bytes())
}

fn parse_implicit(str_: &[u8]) -> Option<(Tm, Tm)> {
    let mut s = str_;
    let mut date_type = PAST;
    for &(p, v) in PREFIXES {
        if starts_with(s, p) {
            date_type = v;
            s = &s[p.len()..];
        }
    }
    let mut num: i64 = 1;
    if date_type != NOW {
        let (n, end) = crate::size::strtoll(s);
        let mut end_of_num = end;
        if end == 0 {
            num = 1;
            for &(f, v) in NUM_CONSTANTS {
                if starts_with(s, f) {
                    num = v;
                    end_of_num += f.len();
                }
            }
        } else {
            num = n;
        }
        s = &s[end_of_num.min(s.len())..];
    }
    let mut kind = None;
    if num > 1 {
        for &(f, k) in PLURAL {
            if s == f.as_bytes() {
                kind = Some(k);
            }
        }
    } else if num == 1 {
        for &(f, k) in SINGULAR {
            if s == f.as_bytes() {
                kind = Some(k);
            }
        }
    } else {
        return None;
    }
    let kind = kind?;
    let tm_now = ctime::localtime(ctime::now());
    let (start, end) = if date_type == NOW {
        (round_down(tm_now, kind), tm_now)
    } else {
        let end = if date_type == PAST_FINAL {
            round_down(tm_now, kind)
        } else {
            tm_now
        };
        (subtract_from_tm(end, kind, num as i32), end)
    };
    Some((start, end))
}

fn cmp_tm(a: &Tm, b: &Tm, interval: Kind) -> std::cmp::Ordering {
    let fields = [
        (a.tm_year, b.tm_year, Kind::Year),
        (a.tm_mon, b.tm_mon, Kind::Month),
        (a.tm_mday, b.tm_mday, Kind::Day),
        (a.tm_hour, b.tm_hour, Kind::Hour),
        (a.tm_min, b.tm_min, Kind::Minute),
    ];
    for (x, y, k) in fields {
        let r = x.cmp(&y);
        if r.is_ne() || interval == k {
            return r;
        }
    }
    a.tm_sec.cmp(&b.tm_sec)
}

fn lower_clamp(tm: &mut Tm, lower: &Tm) {
    if cmp_tm(tm, lower, Kind::Second).is_ge() {
        return;
    }
    tm.tm_year = lower.tm_year;
    tm.tm_mon = lower.tm_mon;
    tm.tm_mday = lower.tm_mday;
    tm.tm_hour = lower.tm_hour;
    tm.tm_min = lower.tm_min;
    tm.tm_sec = lower.tm_sec;
}

fn round_down_to_reference(tm: &mut Tm, reference: &Tm, kind: Kind) -> bool {
    let c = cmp_tm(tm, reference, kind);
    if c.is_lt() {
        return false;
    }
    if c.is_eq() {
        lower_clamp(tm, reference);
    }
    true
}

/// `fsearch_date_time_parse_interval`: the time interval `[start, end)` a date expression means.
pub fn parse_interval(s: &[u8]) -> Option<(i64, i64)> {
    if let Some((mut a, mut b)) = parse_explicit(s).or_else(|| parse_implicit(s)) {
        return Some((unix_from_tm(&mut a), unix_from_tm(&mut b)));
    }
    const FORMATS: &[(&str, Kind)] = &[
        ("%Y-%m-%d %H:%M:%S", Kind::Second),
        ("%y-%m-%d %H:%M:%S", Kind::Second),
        ("%Y-%m-%d %H:%M", Kind::Minute),
        ("%y-%m-%d %H:%M", Kind::Minute),
        ("%Y-%m-%d %H", Kind::Hour),
        ("%y-%m-%d %H", Kind::Hour),
        ("%Y-%m-%d", Kind::Day),
        ("%y-%m-%d", Kind::Day),
        ("%Y-%m", Kind::Month),
        ("%y-%m", Kind::Month),
        ("%Y", Kind::Year),
        ("%y", Kind::Year),
    ];
    let tm_zero = ctime::localtime(0);
    for &(fmt, kind) in FORMATS {
        let mut start = ctime::zero_tm();
        start.tm_mday = 1;
        match ctime::strptime(s, fmt, &mut start) {
            Some([]) => {}
            _ => continue,
        }
        if start.tm_year < tm_zero.tm_year {
            continue;
        }
        start.tm_isdst = -1;
        let mut end = start;
        match kind {
            Kind::Year => end.tm_year += 1,
            Kind::Month => end.tm_mon += 1,
            Kind::Day => end.tm_mday += 1,
            Kind::Hour => end.tm_hour += 1,
            Kind::Minute => end.tm_min += 1,
            Kind::Second => end.tm_sec += 1,
            Kind::Week => continue,
        }
        if !round_down_to_reference(&mut start, &tm_zero, kind)
            && !round_down_to_reference(&mut end, &tm_zero, kind)
        {
            return None;
        }
        return Some((unix_from_tm(&mut start), unix_from_tm(&mut end)));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Windows backend (chrono) agrees with libc on normalization and parsing.
    #[test]
    fn chrono_backend_matches_libc() {
        use super::chrono_ctime as c;
        let mut checked = 0;
        for year in [69, 70, 99, 100, 122, 124, 125, 138] {
            for mon in [-13, -1, 0, 1, 2, 11, 12, 25] {
                for mday in [-40, 0, 1, 15, 28, 29, 30, 31, 32, 60] {
                    for hour in [-25, 0, 5, 23, 24, 49] {
                        let mut a = ctime::zero_tm();
                        a.tm_year = year;
                        a.tm_mon = mon;
                        a.tm_mday = mday;
                        a.tm_hour = hour;
                        a.tm_isdst = -1;
                        let mut b = c::zero_tm();
                        b.tm_year = year;
                        b.tm_mon = mon;
                        b.tm_mday = mday;
                        b.tm_hour = hour;
                        let (ta, tb) = (ctime::mktime(&mut a), c::mktime(&mut b));
                        assert_eq!(ta, tb, "mktime {year} {mon} {mday} {hour}");
                        assert_eq!(
                            (a.tm_year, a.tm_mon, a.tm_mday, a.tm_hour),
                            (b.tm_year, b.tm_mon, b.tm_mday, b.tm_hour)
                        );
                        checked += 1;
                    }
                }
            }
        }
        let fmts = [
            "%Y-%m-%d %H:%M:%S",
            "%y-%m-%d %H:%M",
            "%Y-%m-%d",
            "%y-%m",
            "%Y",
            "%y",
        ];
        for s in [
            "2022",
            "22",
            "2022-01",
            "22-1",
            "22-1-1",
            "22-1-1 12:00:00",
            "2022-13",
            "1960",
            "2022-01-01 12",
            "abc",
            "2022x",
            "69",
            "68",
        ] {
            for f in fmts {
                let (mut a, mut b) = (ctime::zero_tm(), c::zero_tm());
                let ra = ctime::strptime(s.as_bytes(), f, &mut a).map(|r| r.len());
                let rb = c::strptime(s.as_bytes(), f, &mut b).map(|r| r.len());
                assert_eq!(ra, rb, "strptime {s:?} {f}");
                if ra.is_some() {
                    assert_eq!(
                        (a.tm_year, a.tm_mon, a.tm_mday, a.tm_hour, a.tm_min),
                        (b.tm_year, b.tm_mon, b.tm_mday, b.tm_hour, b.tm_min),
                        "{s:?} {f}"
                    );
                }
                checked += 1;
            }
        }
        assert!(checked > 2000);
    }

    #[test]
    fn years_and_months() {
        let (a, b) = parse_interval(b"2022").expect("year");
        assert!(b > a);
        assert!(parse_interval(b"2022-13").is_none());
        assert!(parse_interval(b"abc").is_none());
        assert!(parse_interval(b"today").is_some());
        assert!(parse_interval(b"last3days").is_some());
    }
}
