//! Port of `src/tests/test_time_utils.c`.
use fsearch_core::time::parse_interval;

#[test]
fn parse_time_interval() {
    let cases: &[(&str, bool)] = &[
        ("2000abc", false),
        ("abc2000", false),
        ("abc", false),
        ("today", true),
        ("yesterday", true),
        ("thishour", true),
        ("pastyear", true),
        ("past4year", false),
        ("pastyears", false),
        ("past3years", true),
        ("lastweek", true),
        ("last2weeks", true),
        ("lasttwoweeks", true),
        ("lastweeks", false),
        ("inthelastday", true),
        ("4months", true),
        ("4month", false),
        ("3min", true),
        ("3minutes", true),
        ("2022", true),
        ("22", true),
        ("2022-01", true),
        ("22-01", true),
        ("22-1", true),
        ("22-1-1", true),
        ("22-1-1 12:00:00", true),
        ("2022-01-01 12:00:00", true),
        ("2022-01-01 12:00", true),
        ("2022-01-01 12", true),
        ("2022-01 12:00:00", false),
        ("1960", false),
    ];
    for &(s, ok) in cases {
        assert_eq!(parse_interval(s.as_bytes()).is_some(), ok, "{s}");
    }
}
