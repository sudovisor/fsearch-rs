// Windows: a GUI program (no console window); command-line use attaches to the parent console.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod cli;
mod gui;
mod units;

use std::ffi::OsString;
use std::process::ExitCode;

fn main() -> ExitCode {
    #[cfg(windows)]
    attach_console();
    // `rust-anything app replay` counts fallbacks through this log; this build makes none.
    if let Some(p) = std::env::var_os("RUST_ANYTHING_FALLBACK_LOG") {
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p);
    }
    let argv: Vec<OsString> = std::env::args_os().collect();
    if let Ok(matches) = cli::command().try_get_matches_from(&argv) {
        let (command, sub) = matches.subcommand().unwrap_or((cli::ROOT, &matches));
        if let Some(code) = units::dispatch(command, sub) {
            return code;
        }
    }
    // Options the router does not model (e.g. `-s PATTERN`) belong to the application itself.
    let root = cli::command().get_matches_from([cli::ROOT]);
    units::dispatch(cli::ROOT, &root).unwrap_or(ExitCode::from(1))
}

/// With the `windows` subsystem there is no console; when started from one (command-line use),
/// write to it.
#[cfg(windows)]
fn attach_console() {
    if std::env::args_os().len() > 1 {
        // SAFETY: AttachConsole takes a process id and has no other preconditions; failure (no
        // parent console, e.g. started from Explorer) is harmless and ignored.
        unsafe {
            windows_sys::Win32::System::Console::AttachConsole(
                windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
            );
        }
    }
}
