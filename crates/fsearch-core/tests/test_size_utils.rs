//! Port of `src/tests/test_size_utils.c`.
use fsearch_core::size::parse;

#[test]
fn parse_size() {
    let (fkb, fmb) = (1000i64, 1_000_000i64);
    let (fgb, ftb) = (1000 * fmb, 1_000_000 * fmb);
    let pkb = 1000 - 50 - 1;
    let pmb = fkb * (1000 - 50) - 1;
    let pgb = fmb * (1000 - 50) - 1;
    let ptb = fgb * (1000 - 50) - 1;
    let cases: &[(&str, bool, i64, i64)] = &[
        ("abc", false, 0, 0),
        ("mb", false, 0, 0),
        ("0m", true, 0, pmb),
        ("100", true, 100, 0),
        ("100abc", false, 100, 0),
        ("100k", true, 100 * fkb, pkb),
        ("100K", true, 100 * fkb, pkb),
        ("12mb", true, 12 * fmb, pmb),
        ("12Mb", true, 12 * fmb, pmb),
        ("12mB", true, 12 * fmb, pmb),
        ("123MB", true, 123 * fmb, pmb),
        ("1234GB", true, 1234 * fgb, pgb),
        ("12345TB", true, 12345 * ftb, ptb),
    ];
    for &(s, ok, size, plus) in cases {
        let r = parse(s.as_bytes());
        assert_eq!(r.is_some(), ok, "{s}");
        if let Some((a, b)) = r {
            assert_eq!(a, size, "{s}");
            assert_eq!(b, size + plus, "{s}");
        }
    }
}
