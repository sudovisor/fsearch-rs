//! The recorded behavior of the C original (`tests/app-cases`, see tools/compat/README.md): every
//! case runs the built binary and compares its output byte for byte.
//!
//! The cases were recorded against a generated fixture tree; its location is written as
//! `@HARNESS@` in the case files. This test regenerates the tree (needs `python3`), substitutes
//! its path and replays the cases. Cases that print the checksum of a written database are left
//! out here: the database stores the absolute root path, so its bytes depend on where the tree
//! lives. Their contents are still checked by the matching `scan_*` cases.

use std::path::Path;
use std::process::Command;

const PLACEHOLDER: &str = "@HARNESS@";

#[test]
fn recorded_cases() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let bin = Path::new(env!("CARGO_BIN_EXE_fsearch"));
    // The root folder's full path is searchable; keep the recorded words, without checkout-specific parents.
    let run = std::env::temp_dir()
        .join("@HARNESS@")
        .join("Documents")
        .join("fsearch-rs")
        .join(format!("harness-{:b}", std::process::id()));
    let harness = run.join("harness");
    let cases_dir = run.join("cases");
    let _ = std::fs::remove_dir_all(&run);
    for d in [
        harness.join("fixture"),
        harness.join("out"),
        cases_dir.clone(),
    ] {
        std::fs::create_dir_all(&d).unwrap();
    }

    let tree = harness.join("fixture/tree");
    let made = Command::new("python3")
        .arg(root.join("tools/compat/make_fixture.py"))
        .arg(&tree)
        .status();
    if !made.is_ok_and(|s| s.success()) {
        eprintln!("recorded cases skipped: python3 is needed to generate the fixture tree");
        return;
    }
    // the cases also read the generator itself (as a file that is not a database)
    std::fs::copy(
        root.join("tools/compat/make_fixture.py"),
        harness.join("make_fixture.py"),
    )
    .unwrap();
    let db = Command::new(bin)
        .arg("scanmd5")
        .arg(harness.join("fixture/fixture.db"))
        .arg(&tree)
        .output()
        .unwrap();
    assert!(db.status.success(), "building the fixture database failed");

    let base = harness.to_str().expect("the temp directory path is UTF-8");
    let mut kept = 0;
    for entry in std::fs::read_dir(root.join("tests/app-cases")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|x| x != "toml") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        let read = |ext: &str| std::fs::read_to_string(path.with_extension(ext)).ok();
        let stdout = read("stdout");
        if stdout.as_deref().is_some_and(|s| s.contains(" md5 ")) {
            continue;
        }
        let put = |ext: &str, text: &str| {
            std::fs::write(cases_dir.join(format!("{stem}.{ext}")), text).unwrap()
        };
        let toml = format!(
            "{}\nfs.sandbox = false",
            read("toml")
                .unwrap()
                .replace(PLACEHOLDER, base)
                .replace("fs.sandbox = true", "")
        );
        put("toml", &toml);
        for ext in ["stdout", "stderr"] {
            if let Some(t) = read(ext) {
                put(ext, &t.replace(PLACEHOLDER, base));
            }
        }
        kept += 1;
    }
    assert!(kept > 300, "only {kept} recorded cases found");
    let cases = trycmd::TestCases::new();
    cases
        .register_bin("fsearch", bin)
        .case(cases_dir.join("*.toml"));
    cases.run();
    drop(cases);
    std::fs::remove_dir_all(&run).unwrap();
}
