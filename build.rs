//! Links `compat/glibc_compat.c` on Linux x86_64: older libm symbol versions (see that file).
fn main() {
    println!("cargo:rerun-if-changed=compat/glibc_compat.c");
    println!("cargo:rerun-if-changed=assets/fsearch.ico");
    let target = std::env::var("TARGET").unwrap_or_default();
    if target == "x86_64-unknown-linux-gnu" {
        cc::Build::new()
            .file("compat/glibc_compat.c")
            .compile("glibc_compat");
        for f in ["atan2f", "hypot", "hypotf", "fmod"] {
            println!("cargo:rustc-link-arg=-Wl,--wrap={f}");
        }
        println!("cargo:rustc-link-lib=m");
    }
    if target.contains("windows") {
        windows_resources(&target);
    }
}

/// The program icon and version info, compiled with windres (MinGW) and linked in.
fn windows_resources(target: &str) {
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let v: Vec<&str> = version.split('.').collect();
    let comma = format!(
        "{},{},{},0",
        v.first().unwrap_or(&"0"),
        v.get(1).unwrap_or(&"0"),
        v.get(2).unwrap_or(&"0")
    );
    let ico = format!("{manifest}/assets/fsearch.ico").replace('\\', "/");
    let rc = format!(
        "1 ICON \"{ico}\"\n\
1 VERSIONINFO\nFILEVERSION {comma}\nPRODUCTVERSION {comma}\nBEGIN\n BLOCK \"StringFileInfo\"\n BEGIN\n  BLOCK \"040904B0\"\n  BEGIN\n   \
VALUE \"FileDescription\", \"FSearch\"\n   VALUE \"ProductName\", \"FSearch\"\n   VALUE \"FileVersion\", \"{version}\"\n   \
VALUE \"ProductVersion\", \"{version}\"\n   VALUE \"LegalCopyright\", \"GPL-2.0-or-later\"\n  END\n END\n \
BLOCK \"VarFileInfo\"\n BEGIN\n  VALUE \"Translation\", 0x409, 1200\n END\nEND\n"
    );
    let rc_path = out.join("fsearch.rc");
    std::fs::write(&rc_path, rc).expect("write rc");
    let res = out.join("fsearch-res.o");
    let windres = if target.contains("gnu") {
        "x86_64-w64-mingw32-windres"
    } else {
        "windres"
    };
    let ok = std::process::Command::new(windres)
        .arg(&rc_path)
        .arg("-O")
        .arg("coff")
        .arg("-o")
        .arg(&res)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        println!("cargo:rustc-link-arg-bins={}", res.display());
    } else {
        println!("cargo:warning=windres not found: building without the program icon");
    }
}
